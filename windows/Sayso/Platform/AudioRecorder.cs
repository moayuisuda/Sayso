using NAudio.Wave;
using NAudio.CoreAudioApi;
using NAudio.Wave.SampleProviders;

namespace Sayso.Platform;

internal sealed class AudioRecorder(Action<byte[], int> deliver) : IDisposable
{
    // 语音音量区间，与 macOS 一致（实测：环境底噪约 -54…-45 dB，正常说话约 -38…-22 dB）。
    // 人声很少接近 0 dB，按满刻度映射会把轻声与大声挤在一起；下限压住底噪，上限对应大声说话。
    private const double QuietDecibels = -48, LoudDecibels = -20;
    private WasapiCapture? capture;
    private BufferedWaveProvider? buffer;
    private IWaveProvider? pcm;
    private TaskCompletionSource? stopped;
    private readonly byte[] output = new byte[3200];
    private float level;
    public float Level => Volatile.Read(ref level);

    public void Start()
    {
        if (capture is not null) throw new InvalidOperationException("麦克风已在录音。");
        Volatile.Write(ref level, 0);
        try
        {
            capture = new WasapiCapture();
            buffer = new BufferedWaveProvider(capture.WaveFormat) { ReadFully = false };
            ISampleProvider samples = buffer.ToSampleProvider();
            if (samples.WaveFormat.Channels == 2) samples = new StereoToMonoSampleProvider(samples);
            if (samples.WaveFormat.Channels != 1) throw new InvalidOperationException("麦克风需要单声道或双声道输入。");
            pcm = new SampleToWaveProvider16(new WdlResamplingSampleProvider(samples, 16000));
            stopped = new(TaskCreationOptions.RunContinuationsAsynchronously);
            capture.DataAvailable += (_, e) =>
            {
                buffer.AddSamples(e.Buffer, 0, e.BytesRecorded);
                int count;
                while ((count = pcm.Read(output, 0, output.Length)) > 0)
                {
                    // 与交给核心的同一份 PCM 计算 RMS，按语音音量区间映射到 0…1。
                    double sum = 0;
                    for (var offset = 0; offset < count; offset += 2)
                    {
                        var sample = System.Buffers.Binary.BinaryPrimitives.ReadInt16LittleEndian(output.AsSpan(offset, 2)) / 32768.0;
                        sum += sample * sample;
                    }
                    var decibels = 20 * Math.Log10(Math.Max(Math.Sqrt(sum / (count / 2)), 1e-7));
                    Volatile.Write(ref level, (float)Math.Clamp((decibels - QuietDecibels) / (LoudDecibels - QuietDecibels), 0, 1));
                    deliver(output, count);
                }
            };
            capture.RecordingStopped += (_, e) =>
            {
                if (e.Exception is { } error) stopped.TrySetException(error);
                else stopped.TrySetResult();
            };
            capture.StartRecording();
        }
        catch { capture?.Dispose(); capture = null; throw; }
    }

    public void Stop()
    {
        if (capture is null) return;
        capture.StopRecording();
        try { stopped!.Task.GetAwaiter().GetResult(); }
        finally { capture.Dispose(); capture = null; buffer = null; pcm = null; Volatile.Write(ref level, 0); }
    }

    public void Dispose() => Stop();
}
