using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

namespace Sayso.Platform;

/// <summary>API Key 只存 Windows Credential Manager，settings.json 不保存密钥。</summary>
internal sealed class CredentialStore(string service = "Sayso")
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct Credential
    {
        public uint Flags, Type; public string Target; public string? Comment; public long LastWritten;
        public uint Size; public nint Blob; public uint Persist, AttributeCount; public nint Attributes;
        public string? Alias, User;
    }
    [DllImport("advapi32", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool CredRead(string target, uint type, uint flags, out nint credential);
    [DllImport("advapi32", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool CredWrite(ref Credential credential, uint flags);
    [DllImport("advapi32", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool CredDelete(string target, uint type, uint flags);
    [DllImport("advapi32")] private static extern void CredFree(nint buffer);

    public string? Read(string account)
    {
        if (!CredRead($"{service}/{account}", 1, 0, out var pointer))
        {
            if (Marshal.GetLastWin32Error() == 1168) return null;
            throw new Win32Exception();
        }
        try
        {
            var credential = Marshal.PtrToStructure<Credential>(pointer);
            var bytes = new byte[credential.Size];
            Marshal.Copy(credential.Blob, bytes, 0, bytes.Length);
            return Encoding.Unicode.GetString(bytes);
        }
        finally { CredFree(pointer); }
    }

    public void Write(string account, string? value)
    {
        var target = $"{service}/{account}";
        if (value is null)
        {
            if (!CredDelete(target, 1, 0) && Marshal.GetLastWin32Error() != 1168) throw new Win32Exception();
            return;
        }
        var bytes = Encoding.Unicode.GetBytes(value);
        var pointer = Marshal.AllocCoTaskMem(bytes.Length);
        try
        {
            Marshal.Copy(bytes, 0, pointer, bytes.Length);
            var credential = new Credential { Type = 1, Target = target, User = "Sayso", Size = (uint)bytes.Length, Blob = pointer, Persist = 2 };
            if (!CredWrite(ref credential, 0)) throw new Win32Exception();
        }
        finally { Marshal.FreeCoTaskMem(pointer); }
    }
}
