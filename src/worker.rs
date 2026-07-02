use std::ffi::CStr;
use std::os::raw::c_void;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::RequestContext;
use crate::ffi::*;

unsafe extern "C" fn on_speech_done(_chan: SpeechChannel, ref_con: *mut c_void) {
    let tx = unsafe {
        &*(ref_con as *const tokio::sync::mpsc::Sender<()>)
    };

    tx.blocking_send(()).unwrap();
}

unsafe fn channel_open(voice_name: &str) -> *mut c_void {
    let start_at = Instant::now();

    let mut voice = VoiceSpec {
        creator: 0,
        id: 0,
        instance: 0,
    };

    let cf_voice = unsafe { cfstring_from_str(voice_name) };
    let v_err = unsafe { MakeVoiceSpecForIdentifierString(cf_voice, &mut voice) };
    unsafe { CFRelease(cf_voice as *const _) };

    tracing::debug!(
        "MakeVoiceSpecForIdentifierString('{}') -> {}",
        voice_name,
        v_err
    );

    if v_err != 1 {
        panic!("Voice '{}' not found ({})", voice_name, v_err);
    }

    let voice_ptr = &voice as *const VoiceSpec;

    let mut speech_channel = std::ptr::null_mut();

    let err = unsafe { NewSpeechChannel(voice_ptr, &raw mut speech_channel) };

    tracing::debug!("NewSpeechChannel -> {}", err);

    if err != 0 {
        panic!("NewSpeechChannel: {}", err);
    }

    let cb: SpeechDoneCallBack = on_speech_done;
    let cb_num = unsafe {
        CFNumberCreate(
            std::ptr::null(),
            K_CF_NUMBER_SINT64_TYPE,
            &cb as *const _ as *const c_void,
        )
    };

    let err = unsafe { SetSpeechProperty(speech_channel, kSpeechSpeechDoneCallBack, cb_num) };

    unsafe { CFRelease(cb_num as *const _) };

    tracing::debug!("SetSpeechProperty(callback) -> {}", err);

    if err != 0 {
        panic!("SetSpeechProperty (callback): {}", err);
    }

    tracing::info!("Initialized in {:?}", start_at.elapsed());

    speech_channel
}

unsafe fn synthesize(speech_channel: *mut c_void, text: &str) -> Vec<u8> {
    let start_at = Instant::now();
    let session = uuid::Uuid::new_v4();

    let mut temp_wav = crate::TEMPORARY_DIR.get().unwrap().clone();
    temp_wav.push(format!("say-server_{}.wav", session));

    let mut path_buf = temp_wav.to_str().unwrap().as_bytes().to_vec();

    path_buf.push(0);

    let path_cstr = unsafe { CStr::from_ptr(path_buf.as_ptr() as *const i8) };
    let path_str = path_cstr.to_str().unwrap();
    tracing::debug!("tmpfile {}", path_str);

    let path_cf = unsafe { cfstring_from_str(path_str) };
    let file_url = unsafe {
        CFURLCreateWithFileSystemPath(std::ptr::null(), path_cf, K_CF_URL_POSIX_PATH_STYLE, 0)
    };
    unsafe { CFRelease(path_cf as *const _) };
    if file_url.is_null() {
        panic!("CFURLCreateWithFileSystemPath failed");
    }

    let fmt = AudioStreamBasicDescription {
        m_sample_rate: 22050.0,
        m_format_id: K_AUDIO_FORMAT_LINEAR_PCM,
        m_format_flags: K_AUDIO_FORMAT_FLAG_IS_SIGNED_INTEGER | K_AUDIO_FORMAT_FLAG_IS_PACKED,
        m_bytes_per_packet: 2,
        m_frames_per_packet: 1,
        m_bytes_per_frame: 2,
        m_channels_per_frame: 1,
        m_bits_per_channel: 16,
        m_reserved: 0,
    };

    let mut ext_file: ExtAudioFileRef = std::ptr::null_mut();
    let err = unsafe {
        ExtAudioFileCreateWithURL(
            file_url,
            K_WAVE_FILE_TYPE,
            &fmt,
            std::ptr::null(),
            1,
            &mut ext_file,
        )
    };
    tracing::debug!("ExtAudioFileCreateWithURL -> {}", err);
    unsafe { CFRelease(file_url as *const _) };
    if err != 0 {
        panic!("ExtAudioFileCreateWithURL: {}", err);
    }

    let ext_file_val = ext_file as i64;
    let ext_num = unsafe {
        CFNumberCreate(
            std::ptr::null(),
            K_CF_NUMBER_SINT64_TYPE,
            &ext_file_val as *const _ as *const c_void,
        )
    };
    let err =
        unsafe { SetSpeechProperty(speech_channel, kSpeechOutputToExtAudioFileProperty, ext_num) };
    unsafe { CFRelease(ext_num as *const _) };
    tracing::debug!("SetSpeechProperty(extfile) -> {}", err);
    if err != 0 {
        unsafe { ExtAudioFileDispose(ext_file) };
        panic!("SetSpeechProperty (extfile): {}", err);
    }

    let cf_text = unsafe { cfstring_from_str(text) };

    let (tx, mut rx): (_, tokio::sync::mpsc::Receiver<()>) = tokio::sync::mpsc::channel(1);

    let tx_ptr = &tx as *const _ as *const c_void as i64;

    let ref_con_num = unsafe {
        CFNumberCreate(
            std::ptr::null(),
            K_CF_NUMBER_SINT64_TYPE,
            &tx_ptr as *const _ as *const c_void,
        )
    };

    let err = unsafe { SetSpeechProperty(speech_channel, kSpeechRefConProperty, ref_con_num) };
    unsafe { CFRelease(ref_con_num as *const _) };
    tracing::debug!("SetSpeechProperty(refCon) -> {}", err);
    if err != 0 {
        unsafe { ExtAudioFileDispose(ext_file) };
        panic!("SetSpeechProperty(refCon): {}", err);
    }

    let err = unsafe { SpeakCFString(speech_channel, cf_text, std::ptr::null_mut()) };
    unsafe { CFRelease(cf_text as *const _) };
    tracing::debug!("SpeakCFString -> {} (waiting...)", err);
    if err != 0 {
        unsafe { ExtAudioFileDispose(ext_file) };
        panic!("SpeakCFString: {}", err);
    }

    let _ = rx.blocking_recv();
    tracing::debug!("rx.blocking_recv() returned (speech done)");
    drop(tx);

    unsafe { ExtAudioFileDispose(ext_file) };
    tracing::debug!("ExtAudioFileDispose");

    let file_bytes = std::fs::read(path_str).unwrap();

    tracing::debug!("file read done ({} bytes)", file_bytes.len());

    let _ = std::fs::remove_file(path_str);

    tracing::info!("Synthesis: {:?}", start_at.elapsed());

    file_bytes
}

pub fn run(voice_id: &str, mut rx: mpsc::Receiver<RequestContext>) {
    let speech_channel = unsafe { channel_open(voice_id) };

    while let Some(msg) = rx.blocking_recv() {
        msg.writeback
            .send(unsafe { synthesize(speech_channel, &msg.text) })
            .unwrap();
    }
}
