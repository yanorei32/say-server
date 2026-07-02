use std::ffi::c_void;
use std::os::raw::c_char;

pub type CFTypeRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFNumberRef = *const c_void;
pub type CFURLRef = *const c_void;
pub type CFIndex = i64;
pub type OSStatus = i32;
pub type OSType = u32;
pub type Boolean = u8;

pub type SpeechChannel = *mut c_void;
pub type ExtAudioFileRef = *mut c_void;

pub type SpeechDoneCallBack = unsafe extern "C" fn(SpeechChannel, *mut c_void);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VoiceSpec {
    pub creator: OSType,
    pub id: OSType,
    pub instance: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AudioStreamBasicDescription {
    pub m_sample_rate: f64,
    pub m_format_id: u32,
    pub m_format_flags: u32,
    pub m_bytes_per_packet: u32,
    pub m_frames_per_packet: u32,
    pub m_bytes_per_frame: u32,
    pub m_channels_per_frame: u32,
    pub m_bits_per_channel: u32,
    pub m_reserved: u32,
}

pub const K_SPEECH_VOICE_ATTR_SELECTOR: u32 = 0x61747472;
pub const K_CF_ENCODING_UTF8: u32 = 0x08000100;
pub const K_CF_NUMBER_SINT64_TYPE: i64 = 4;
pub const K_CF_URL_POSIX_PATH_STYLE: i64 = 0;

pub const K_WAVE_FILE_TYPE: u32 = 0x57415645;
pub const K_AUDIO_FORMAT_LINEAR_PCM: u32 = 0x6C70636D;
pub const K_AUDIO_FORMAT_FLAG_IS_SIGNED_INTEGER: u32 = 0x4;
pub const K_AUDIO_FORMAT_FLAG_IS_PACKED: u32 = 0x8;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    pub static kSpeechVoiceName: CFStringRef;
    pub static kSpeechVoiceLocaleIdentifier: CFStringRef;
    pub static kSpeechVoiceDemoText: CFStringRef;
    pub static kSpeechSpeechDoneCallBack: CFStringRef;
    pub static kSpeechOutputToExtAudioFileProperty: CFStringRef;
    pub static kSpeechRefConProperty: CFStringRef;

    pub fn CopySpeechSynthesisVoicesForMode(mode: CFArrayRef) -> CFArrayRef;
    pub fn MakeVoiceSpecForIdentifierString(
        identifier: CFStringRef,
        voice: *mut VoiceSpec,
    ) -> OSStatus;
    pub fn GetVoiceInfo(
        voice: *const VoiceSpec,
        selector: u32,
        info: *mut CFDictionaryRef,
    ) -> OSStatus;
    pub fn NewSpeechChannel(
        voice_spec: *const VoiceSpec,
        channel: *mut SpeechChannel,
    ) -> OSStatus;
    pub fn SetSpeechProperty(
        channel: SpeechChannel,
        property: CFStringRef,
        value: CFTypeRef,
    ) -> OSStatus;
    pub fn SpeakCFString(
        channel: SpeechChannel,
        string: CFStringRef,
        ref_con: *mut c_void,
    ) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> *const c_void;
    pub fn CFDictionaryGetValue(dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
    pub fn CFStringCreateWithCString(
        alloc: CFTypeRef,
        c_str: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    pub fn CFStringGetCString(
        str: CFStringRef,
        buffer: *mut c_char,
        bufferSize: usize,
        encoding: u32,
    ) -> Boolean;
    pub fn CFNumberCreate(
        alloc: CFTypeRef,
        the_type: i64,
        value_ptr: *const c_void,
    ) -> CFNumberRef;
    pub fn CFURLCreateWithFileSystemPath(
        alloc: CFTypeRef,
        file_path: CFStringRef,
        path_style: i64,
        is_directory: Boolean,
    ) -> CFURLRef;
    pub fn CFRelease(cf: *const c_void);
}

pub unsafe fn cfstring_from_str(s: &str) -> CFStringRef {
    unsafe {
        CFStringCreateWithCString(
            std::ptr::null(),
            s.as_ptr() as *const i8,
            K_CF_ENCODING_UTF8,
        )
    }
}

pub unsafe fn cfstring_to_string(s: CFStringRef) -> String {
    if s.is_null() {
        return String::new();
    }
    let mut buf = [0u8; 256];
    let ok = unsafe {
        CFStringGetCString(s, buf.as_mut_ptr() as *mut i8, buf.len(), K_CF_ENCODING_UTF8)
    };
    if ok != 0 {
        unsafe { std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8) }
            .to_string_lossy()
            .into_owned()
    } else {
        String::new()
    }
}

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    pub fn ExtAudioFileCreateWithURL(
        url: CFURLRef,
        file_type: u32,
        format: *const AudioStreamBasicDescription,
        property_flags: *const c_void,
        number_frames: u32,
        out_ext_audio_file: *mut ExtAudioFileRef,
    ) -> OSStatus;
    pub fn ExtAudioFileDispose(ext_audio_file: ExtAudioFileRef) -> OSStatus;
}
