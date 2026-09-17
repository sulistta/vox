use vox_audio_capture::{native_capture_backend, CaptureBackend};

/// The desktop UI must not open the microphone until it can turn the captured
/// audio into an editable transcript. Capture and transcription are separate
/// capabilities, so a detected capture program is intentionally not treated
/// as a working voice feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCapability {
    capture_backend: Option<CaptureBackend>,
}

impl VoiceCapability {
    pub fn detect() -> Self {
        Self::without_transcriber(native_capture_backend().ok())
    }

    fn without_transcriber(capture_backend: Option<CaptureBackend>) -> Self {
        Self { capture_backend }
    }

    pub fn is_enabled(&self) -> bool {
        false
    }

    pub fn button_label(&self) -> &'static str {
        "Voz indisponível"
    }

    pub fn summary(&self) -> String {
        match self.capture_backend {
            Some(backend) => format!(
                "Voz indisponível: o backend de captura {} foi detectado, mas o Vox ainda não tem uma transcrição de fala conectada.",
                backend.label()
            ),
            None => "Voz indisponível: nenhum backend de captura nativa foi detectado e o Vox ainda não tem uma transcrição de fala conectada.".into(),
        }
    }

    pub fn detail(&self) -> &'static str {
        "O microfone não será aberto até existir uma integração de STT que produza um texto revisável. Use texto para enviar o pedido nesta versão."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_detection_does_not_claim_that_voice_is_ready() {
        let capability = VoiceCapability::without_transcriber(Some(CaptureBackend::PipeWire));
        assert!(!capability.is_enabled());
        assert_eq!(capability.button_label(), "Voz indisponível");
        assert!(capability.summary().contains("PipeWire"));
        assert!(capability.detail().contains("não será aberto"));
    }

    #[test]
    fn missing_capture_backend_has_a_specific_explanation() {
        let capability = VoiceCapability::without_transcriber(None);
        assert!(!capability.is_enabled());
        assert!(capability.summary().contains("nenhum backend"));
    }
}
