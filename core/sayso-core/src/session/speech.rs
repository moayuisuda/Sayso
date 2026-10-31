//! 自动朗读与按钮朗读共用的错误呈现，保持候选内容仍由 core 管理。

use std::sync::Arc;

use super::{hold, Phase, Session, Surface, APP_SOURCE};
use crate::app::Inner;
use crate::l10n::tr;
use crate::protocol::{Block, Language, Tone};

const SPEECH_NOTICE: &str = "speech";

impl Inner {
    pub(crate) fn speak(self: &Arc<Self>, text: String, language: Language, surface_id: Option<&str>) {
        let is_current = |session: &Session| {
            !session.is_active()
                && surface_id.is_none_or(|id| matches!(&session.phase, Phase::Presenting(surface) if surface.id == id))
        };
        if !is_current(&self.session.lock().unwrap()) {
            return;
        }
        let result = self.platform.speak(text, language, self.settings().speech_rate);
        self.update_session(|session| {
            // 迟到的扩展结果不能把播放错误写进新会话，也不能打断录音。
            if !is_current(session) {
                return;
            }
            match result {
                Ok(()) => {
                    if let Phase::Presenting(surface) = &mut session.phase {
                        surface.remove(SPEECH_NOTICE, APP_SOURCE);
                    }
                }
                Err(error) => {
                    let message = tr(&format!("朗读失败：{error}"), &format!("Playback failed: {error}"));
                    let block = Block::status(SPEECH_NOTICE, message, Tone::Error, "speaker.slash.fill");
                    if !matches!(session.phase, Phase::Presenting(_)) {
                        self.present(session, Surface::new(None, hold::FAILURE));
                    }
                    if let Phase::Presenting(surface) = &mut session.phase {
                        surface.upsert(block, APP_SOURCE, None);
                    }
                }
            }
        });
    }
}
