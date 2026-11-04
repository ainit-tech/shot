//! Video files through Media Foundation, a part of Windows: nothing of it is built into the
//! program. A recording goes frame by frame into an MP4 file of H.264, on the graphics card where
//! it can; for a GIF or a shorter video the file is read back frame by frame. Its libraries are
//! looked for when first needed: the N editions of Windows come without them, and the program
//! has to start there all the same.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::null_mut;
use std::sync::OnceLock;

use windows::Win32::Foundation::E_NOTIMPL;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSinkWriter, IMFSourceReader, MF_MT_AVG_BITRATE,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
    MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFMediaType_Video, MFSTARTUP_FULL, MFVideoArea, MFVideoFormat_H264,
    MFVideoFormat_RGB32, MFVideoInterlace_Progressive,
};
use windows_core::{HRESULT, HSTRING, Interface, PCWSTR, Result};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows_sys::w;

use crate::text::tr;

/// The frames a second of a recording, and how many of Media Foundation's units of time, a tenth
/// of a microsecond each, a second has.
pub const FRAMES: u32 = 30;
pub const SECOND: i64 = 10_000_000;
/// The bits of video for a pixel of a frame: a screen is mostly still, and sharp text needs them
/// where it moves.
const BITS: f32 = 0.2;
/// Writes the bitrate's limits, in bits a second.
const LEAST_RATE: f32 = 1_000_000.0;
const MOST_RATE: f32 = 40_000_000.0;

/// Where a function of Media Foundation puts what it makes.
type Made = *mut *mut c_void;

/// The functions of Media Foundation, once found.
struct Foundation {
    attributes: unsafe extern "system" fn(Made, u32) -> HRESULT,
    media_type: unsafe extern "system" fn(Made) -> HRESULT,
    memory_buffer: unsafe extern "system" fn(u32, Made) -> HRESULT,
    sample: unsafe extern "system" fn(Made) -> HRESULT,
    sink_writer: unsafe extern "system" fn(PCWSTR, *mut c_void, *mut c_void, Made) -> HRESULT,
    source_reader: unsafe extern "system" fn(PCWSTR, *mut c_void, Made) -> HRESULT,
}

/// A function of a library, by its name with a zero at the end.
unsafe fn function<F: Copy>(library: HMODULE, name: &str) -> Option<F> {
    let found = unsafe { GetProcAddress(library, name.as_ptr()) }?;
    Some(unsafe { std::mem::transmute_copy(&found) })
}

/// Finds Media Foundation and starts it for the program, once: it stays for as long as the
/// program.
fn foundation() -> Result<&'static Foundation> {
    static FOUNDATION: OnceLock<Option<Foundation>> = OnceLock::new();
    let found = FOUNDATION.get_or_init(|| unsafe {
        let platform = LoadLibraryExW(w!("mfplat.dll"), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        let files = LoadLibraryExW(w!("mfreadwrite.dll"), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        if platform.is_null() || files.is_null() {
            return None;
        }
        let start: unsafe extern "system" fn(u32, u32) -> HRESULT = function(platform, "MFStartup\0")?;
        start(MF_VERSION, MFSTARTUP_FULL).ok().ok()?;
        Some(Foundation {
            attributes: function(platform, "MFCreateAttributes\0")?,
            media_type: function(platform, "MFCreateMediaType\0")?,
            memory_buffer: function(platform, "MFCreateMemoryBuffer\0")?,
            sample: function(platform, "MFCreateSample\0")?,
            sink_writer: function(files, "MFCreateSinkWriterFromURL\0")?,
            source_reader: function(files, "MFCreateSourceReaderFromURL\0")?,
        })
    });
    found.as_ref().ok_or_else(|| windows_core::Error::new(E_NOTIMPL, tr("This edition of Windows has no Media Feature Pack")))
}

/// What a function of Media Foundation makes.
unsafe fn made<T: Interface>(make: impl FnOnce(Made) -> HRESULT) -> Result<T> {
    let mut made = null_mut();
    make(&mut made).ok()?;
    if made.is_null() {
        return Err(windows_core::Error::empty());
    }
    Ok(unsafe { T::from_raw(made) })
}

/// Two numbers as Media Foundation keeps them in one: a size or a fraction.
fn pair(high: u32, low: u32) -> u64 {
    (high as u64) << 32 | low as u64
}

fn attributes(count: u32) -> Result<IMFAttributes> {
    let foundation = foundation()?;
    unsafe { made(|place| (foundation.attributes)(place, count)) }
}

fn video_type(subtype: &windows_core::GUID) -> Result<IMFMediaType> {
    let foundation = foundation()?;
    unsafe {
        let kind: IMFMediaType = made(|place| (foundation.media_type)(place))?;
        kind.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        kind.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        Ok(kind)
    }
}

/// An MP4 file being written.
pub struct Writer {
    sink: IMFSinkWriter,
    stream: u32,
    width: usize,
    height: usize,
}

impl Writer {
    /// A new file of frames of this size, both of which H.264 wants even.
    pub fn create(path: &Path, width: u32, height: u32) -> Result<Writer> {
        let foundation = foundation()?;
        unsafe {
            let settings = attributes(1)?;
            settings.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            let path = HSTRING::from(path);
            let sink: IMFSinkWriter =
                made(|place| (foundation.sink_writer)(PCWSTR(path.as_ptr()), null_mut(), settings.as_raw(), place))?;
            let frame = |kind: &IMFMediaType| -> Result<()> {
                kind.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                kind.SetUINT64(&MF_MT_FRAME_SIZE, pair(width, height))?;
                kind.SetUINT64(&MF_MT_FRAME_RATE, pair(FRAMES, 1))?;
                kind.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1))
            };
            let output = video_type(&MFVideoFormat_H264)?;
            let rate = (width as f32 * height as f32 * FRAMES as f32 * BITS).clamp(LEAST_RATE, MOST_RATE);
            output.SetUINT32(&MF_MT_AVG_BITRATE, rate as u32)?;
            frame(&output)?;
            let stream = sink.AddStream(&output)?;
            let input = video_type(&MFVideoFormat_RGB32)?;
            frame(&input)?;
            // Positive: the rows come from the top.
            input.SetUINT32(&MF_MT_DEFAULT_STRIDE, width * 4)?;
            sink.SetInputMediaType(stream, &input, None)?;
            sink.BeginWriting()?;
            Ok(Writer { sink, stream, width: width as usize, height: height as usize })
        }
    }

    /// Adds a frame: rows from the top, four bytes a pixel, blue, green, red and one ignored, and
    /// `stride` bytes from row to row. `time` is when it comes in the video and `length` how long
    /// it stays, both in Media Foundation's units.
    pub fn write(&mut self, pixels: &[u8], stride: usize, time: i64, length: i64) -> Result<()> {
        let row = self.width * 4;
        let size = (row * self.height) as u32;
        let foundation = foundation()?;
        unsafe {
            let buffer: IMFMediaBuffer = made(|place| (foundation.memory_buffer)(size, place))?;
            let mut target = std::ptr::null_mut();
            buffer.Lock(&mut target, None, None)?;
            for (y, line) in pixels.chunks(stride).take(self.height).enumerate() {
                std::ptr::copy_nonoverlapping(line.as_ptr(), target.add(y * row), row);
            }
            buffer.Unlock()?;
            buffer.SetCurrentLength(size)?;
            let sample: IMFSample = made(|place| (foundation.sample)(place))?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(time)?;
            sample.SetSampleDuration(length)?;
            self.sink.WriteSample(self.stream, &sample)
        }
    }

    /// Ends the file.
    pub fn finish(self) -> Result<()> {
        unsafe { self.sink.Finalize() }
    }
}

/// An MP4 file being read, frame by frame.
pub struct Reader {
    reader: IMFSourceReader,
    pub width: usize,
    pub height: usize,
    /// The rows a frame has in memory, more than the picture's when H.264 wants a multiple of 16.
    rows: usize,
    /// The bytes from row to row: below zero when the rows come from the bottom.
    stride: i32,
}

/// The first video stream of a file, as the reader's calls name it.
const STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

impl Reader {
    pub fn open(path: &Path) -> Result<Reader> {
        let foundation = foundation()?;
        unsafe {
            let settings = attributes(1)?;
            // The plain conversion takes 36 ms for a frame of 1920 by 1080, this one 9.
            settings.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;
            let path = HSTRING::from(path);
            let reader: IMFSourceReader = made(|place| (foundation.source_reader)(PCWSTR(path.as_ptr()), settings.as_raw(), place))?;
            let wanted = video_type(&MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(STREAM, None, &wanted)?;
            let mut made = Reader { reader, width: 0, height: 0, rows: 0, stride: 0 };
            made.learn()?;
            Ok(made)
        }
    }

    /// Learns how the frames come. The decoder only finds out with the first frame how wide its
    /// rows really are, and says so then.
    fn learn(&mut self) -> Result<()> {
        unsafe {
            let current = self.reader.GetCurrentMediaType(STREAM)?;
            let size = current.GetUINT64(&MF_MT_FRAME_SIZE)?;
            let (columns, rows) = ((size >> 32) as usize, (size & 0xFFFF_FFFF) as usize);
            self.rows = rows;
            self.stride = current.GetUINT32(&MF_MT_DEFAULT_STRIDE).map_or(columns as i32 * 4, |stride| stride as i32);
            let mut shown = MFVideoArea::default();
            let bytes = std::slice::from_raw_parts_mut((&raw mut shown).cast::<u8>(), size_of::<MFVideoArea>());
            (self.width, self.height) = match current.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, bytes, None) {
                Ok(()) => (shown.Area.cx as usize, shown.Area.cy as usize),
                Err(_) => (columns, rows),
            };
            Ok(())
        }
    }

    /// The next frame, when it comes in the video and its pixels: rows from the top, four bytes
    /// a pixel. Nothing past the end of the file.
    pub fn next_frame(&mut self) -> Result<Option<(i64, Vec<u8>)>> {
        loop {
            let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
            unsafe { self.reader.ReadSample(STREAM, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample))? };
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                self.learn()?;
            }
            let Some(sample) = sample else { continue };
            let row = self.width * 4;
            let mut pixels = vec![0u8; row * self.height];
            unsafe {
                let buffer = sample.ConvertToContiguousBuffer()?;
                let (mut bytes, mut length) = (std::ptr::null_mut(), 0u32);
                buffer.Lock(&mut bytes, None, Some(&mut length))?;
                let source = std::slice::from_raw_parts(bytes, length as usize);
                let step = self.stride.unsigned_abs() as usize;
                for (y, line) in pixels.chunks_exact_mut(row).enumerate() {
                    let from = if self.stride < 0 { (self.rows - 1 - y) * step } else { y * step };
                    if from + row <= source.len() {
                        line.copy_from_slice(&source[from..from + row]);
                    }
                }
                buffer.Unlock()?;
            }
            return Ok(Some((time, pixels)));
        }
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

    use super::*;

    /// A frame with a white square going across a dark ground, its top left corner red.
    fn frame(width: usize, height: usize, n: usize) -> Vec<u8> {
        let mut pixels = vec![40u8; width * height * 4];
        let at = n * 8 % (width - 64);
        for y in 32..96 {
            for x in at..at + 64 {
                pixels[(y * width + x) * 4..(y * width + x) * 4 + 3].copy_from_slice(&[250, 250, 250]);
            }
        }
        for y in 0..24 {
            for x in 0..24 {
                pixels[(y * width + x) * 4..(y * width + x) * 4 + 3].copy_from_slice(&[0, 0, 255]);
            }
        }
        pixels
    }

    /// Writes frames of a size into a file and reads them back.
    fn read_back(folder: &Path, width: usize, height: usize, frames: usize) {
        let path = folder.join(format!("{width}x{height}.mp4"));
        let mut writer = Writer::create(&path, width as u32, height as u32).unwrap();
        let length = SECOND / FRAMES as i64;
        for n in 0..frames {
            writer.write(&frame(width, height, n), width * 4, n as i64 * length, length).unwrap();
        }
        writer.finish().unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 1000);
        let mut reader = Reader::open(&path).unwrap();
        let (mut count, mut last) = (0, 0);
        while let Some((time, pixels)) = reader.next_frame().unwrap() {
            assert_eq!((reader.width, reader.height), (width, height));
            assert!(time >= last);
            last = time;
            count += 1;
            // The red corner is at the top left: the rows are the right way up, and the colours
            // are the frame's.
            let at = |x: usize, y: usize| &pixels[(y * width + x) * 4..(y * width + x) * 4 + 3];
            let (corner, bottom) = (at(8, 8), at(8, height - 8));
            assert!(corner[2] > 200 && corner[0] < 60 && corner[1] < 60, "{corner:?}");
            assert!(bottom[2] < 90, "{bottom:?}");
            // The rows are as wide as the picture.
            assert!(at(width - 8, 8)[2] < 90, "{:?}", at(width - 8, 8));
        }
        assert!(count + 2 >= frames, "{count} frames of {frames}");
    }

    #[test]
    fn a_file_written_reads_back_right_way_up() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        let folder = std::env::temp_dir().join(format!("ainitshot-video-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        // Sizes no multiple of 16, as a selection's, and the size of a screen.
        read_back(&folder, 642, 362, 45);
        read_back(&folder, 1920, 1080, 12);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
