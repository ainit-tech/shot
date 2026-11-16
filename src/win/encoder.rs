//! The graphics card's own encoder, for a recording (decision 17). The picture of the screen stays
//! on the card from the moment it is read until it is packed: the card's video processor turns
//! its colours into those of a video, NV12, and the card's encoder, NVIDIA's, AMD's or Intel's,
//! whichever Media Foundation finds there, packs them into H.264. The processor only says what to
//! do, so a busy processor no longer makes a recording jerky. NVIDIA's encoder has been seen to
//! crash the process as it is let go, now and then: a recording runs in a process of its own,
//! which ends without letting go of anything.

use std::mem::ManuallyDrop;
use std::path::Path;
use std::ptr::null_mut;

use windows::Win32::Foundation::{E_NOTIMPL, HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_SDK_VERSION, D3D11_TEX2D_ARRAY_VPOV, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_COLOR_SPACE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT, D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_OPTIMAL_QUALITY, D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2DARRAY, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    ID3D11VideoContext, ID3D11VideoContext1, ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncCommonQuality, CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVGOPSize, IMF2DBuffer, IMFAttributes, IMFDXGIBuffer, IMFDXGIDeviceManager, IMFMediaType,
    IMFSinkWriter, IMFSinkWriterEx, IMFVideoSampleAllocatorEx, MF_E_SAMPLEALLOCATOR_EMPTY, MF_MT_AVG_BITRATE, MF_MT_MPEG2_PROFILE, MF_MT_TRANSFER_FUNCTION,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SA_D3D11_BINDFLAGS,
    MF_SINK_WRITER_D3D_MANAGER, MFNominalRange_16_235, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_HARDWARE_URL_Attribute,
    MFT_ENUM_HARDWARE_VENDOR_ID_Attribute, MFT_FRIENDLY_NAME_Attribute, MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoPrimaries_BT709,
    MFVideoTransferMatrix_BT709, MFVideoTransFunc_709, eAVEncCommonRateControlMode_Quality, eAVEncCommonRateControlMode_UnconstrainedVBR,
    eAVEncH264VProfile_High,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows_core::{GUID, HSTRING, Interface, PCWSTR, PWSTR, Result};

use super::video;

/// A key frame every two seconds: a cut or a jump in the player finds one near.
const KEY_SECONDS: u32 = 2;
/// The quality the encoder keeps, of 100: on screens it is as good as the processor's encoder,
/// with files half as large.
const QUALITY: u32 = 90;
/// The frames the encoder may hold at once, waiting to be packed: past them, a frame is left out.
const FIRST_FRAMES: u32 = 2;
const MOST_FRAMES: u32 = 8;

/// A device of a graphics card for video, on `adapter` or on the first card: the encoder works
/// with it from threads of its own.
pub fn video_device(adapter: Option<&IDXGIAdapter1>) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let first;
    let adapter = match adapter {
        Some(adapter) => adapter,
        None => {
            let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
            first = unsafe { factory.EnumAdapters1(0)? };
            &first
        }
    };
    let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
    let (mut device, mut context) = (None, None);
    unsafe {
        D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device: ID3D11Device = device.ok_or_else(windows_core::Error::empty)?;
    let context = context.ok_or_else(windows_core::Error::empty)?;
    let _ = unsafe { device.cast::<ID3D11Multithread>()?.SetMultithreadProtected(true) };
    Ok((device, context))
}

/// A picture of the card in the colours of the screen, of a size: one that can be drawn on and
/// read by the video processor.
pub fn picture(device: &ID3D11Device, (width, height): (u32, u32)) -> Result<ID3D11Texture2D> {
    let description = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&description, None, Some(&mut texture))? };
    texture.ok_or_else(windows_core::Error::empty)
}

/// An MP4 file that the graphics card writes. Its sink writer, with the card's encoder in it, is
/// never let go, whatever becomes of the writer: NVIDIA's encoder has crashed the process as it
/// was let go. The process of a recording ends instead.
pub struct CardWriter {
    sink: ManuallyDrop<IMFSinkWriter>,
    stream: u32,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    video: ID3D11VideoContext,
    video_device: ID3D11VideoDevice,
    processors: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    allocator: IMFVideoSampleAllocatorEx,
    /// The views the video processor reads the pictures through and writes the frames through,
    /// by the picture.
    inputs: Vec<(usize, ID3D11VideoProcessorInputView)>,
    outputs: Vec<((usize, u32), ID3D11VideoProcessorOutputView)>,
    /// Where a picture from memory goes on the card.
    upload: Option<ID3D11Texture2D>,
    /// The size of the pictures, which the video processor makes that of the video.
    from: (u32, u32),
    /// The encoder, as it names itself.
    encoder: String,
}

impl CardWriter {
    /// A new file of frames of `size`, which H.264 wants even, `rate` of them a second, from
    /// pictures of `from` on the card of `device`. It fails if the card has no encoder: the
    /// processor's is for that.
    pub fn create(path: &Path, device: &ID3D11Device, from: (u32, u32), size: (u32, u32), rate: u32) -> Result<CardWriter> {
        let foundation = video::foundation()?;
        let (Some(device_manager), Some(sample_allocator)) = (foundation.device_manager, foundation.sample_allocator) else {
            return Err(E_NOTIMPL.into());
        };
        let (width, height) = size;
        unsafe {
            let mut token = 0;
            let manager: IMFDXGIDeviceManager = video::made(|place| device_manager(&mut token, place))?;
            manager.ResetDevice(device, token)?;
            let settings = video::attributes(2)?;
            settings.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &manager)?;
            settings.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            let wide = HSTRING::from(path);
            let sink: ManuallyDrop<IMFSinkWriter> = ManuallyDrop::new(video::made(|place| {
                (foundation.sink_writer)(PCWSTR(wide.as_ptr()), null_mut(), settings.as_raw(), place)
            })?);
            // The colours of BT.709 at every size: those Media Foundation and the browser take a
            // video to have when it doesn't say, and the encoders don't.
            let colours = |kind: &IMFMediaType| -> Result<()> {
                kind.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
                kind.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
                kind.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)?;
                kind.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)
            };
            let output = video::video_type(&MFVideoFormat_H264)?;
            video::frame_of(&output, width, height, rate)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, video::bitrate(width, height))?;
            colours(&output)?;
            // The encoders begin with the plainest kind of H.264 otherwise, and files twice as large.
            output.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)?;
            let stream = sink.AddStream(&output)?;
            let input = video::video_type(&MFVideoFormat_NV12)?;
            video::frame_of(&input, width, height, rate)?;
            colours(&input)?;
            // A screen is mostly still: the encoder keeps a quality, and its bits go where it moves.
            // One that can't keep a quality keeps a bitrate on average, and one that takes no
            // settings goes its own way.
            let mut taken = Err(windows_core::Error::empty());
            for control in [Some(eAVEncCommonRateControlMode_Quality), Some(eAVEncCommonRateControlMode_UnconstrainedVBR), None] {
                let Some(control) = control else {
                    taken = sink.SetInputMediaType(stream, &input, None);
                    break;
                };
                let parameters = video::attributes(3)?;
                parameters.SetUINT32(&CODECAPI_AVEncCommonRateControlMode, control.0 as u32)?;
                parameters.SetUINT32(&CODECAPI_AVEncCommonQuality, QUALITY)?;
                parameters.SetUINT32(&CODECAPI_AVEncMPVGOPSize, KEY_SECONDS * rate)?;
                taken = sink.SetInputMediaType(stream, &input, &parameters);
                if taken.is_ok() {
                    break;
                }
            }
            taken?;
            // The encoder of Windows would take the pictures off the card one by one, slower than
            // the processor's own way.
            let encoder = card_encoder(&sink, stream).ok_or_else(|| windows_core::Error::new(E_NOTIMPL, "The graphics card has no encoder"))?;

            let context = device.GetImmediateContext()?;
            let video: ID3D11VideoContext = context.cast()?;
            let video_device: ID3D11VideoDevice = device.cast()?;
            let frames = DXGI_RATIONAL { Numerator: rate, Denominator: 1 };
            let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: frames,
                InputWidth: from.0,
                InputHeight: from.1,
                OutputFrameRate: frames,
                OutputWidth: width,
                OutputHeight: height,
                Usage: D3D11_VIDEO_USAGE_OPTIMAL_QUALITY,
            };
            let processors = video_device.CreateVideoProcessorEnumerator(&content)?;
            let input_ok = processors.CheckVideoProcessorFormat(DXGI_FORMAT_B8G8R8A8_UNORM)? & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0 as u32;
            let output_ok = processors.CheckVideoProcessorFormat(DXGI_FORMAT_NV12)? & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32;
            if input_ok == 0 || output_ok == 0 {
                return Err(E_NOTIMPL.into());
            }
            let processor = video_device.CreateVideoProcessor(&processors, 0)?;
            let whole = |(width, height): (u32, u32)| RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
            video.VideoProcessorSetStreamFrameFormat(&processor, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
            video.VideoProcessorSetStreamSourceRect(&processor, 0, true, Some(&whole(from)));
            video.VideoProcessorSetStreamDestRect(&processor, 0, true, Some(&whole(size)));
            video.VideoProcessorSetOutputTargetRect(&processor, true, Some(&whole(size)));
            // No sharpening or smoothing of its own: text stays as it is.
            video.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
            match video.cast::<ID3D11VideoContext1>() {
                Ok(newer) => {
                    newer.VideoProcessorSetStreamColorSpace1(&processor, 0, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
                    newer.VideoProcessorSetOutputColorSpace1(&processor, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709);
                }
                Err(_) => {
                    // Full RGB in; out, BT.709 in the studio range, 16 to 235.
                    let screen = D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 0 };
                    let film = D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 1 << 2 | 1 << 4 };
                    video.VideoProcessorSetStreamColorSpace(&processor, 0, &screen);
                    video.VideoProcessorSetOutputColorSpace(&processor, &film);
                }
            }

            let allocator: IMFVideoSampleAllocatorEx = video::made(|place| sample_allocator(&IMFVideoSampleAllocatorEx::IID, place))?;
            allocator.SetDirectXManager(&manager)?;
            let wanted = video::attributes(1)?;
            wanted.SetUINT32(&MF_SA_D3D11_BINDFLAGS, D3D11_BIND_RENDER_TARGET.0 as u32)?;
            allocator.InitializeSampleAllocatorEx(FIRST_FRAMES, MOST_FRAMES, &wanted, &input)?;
            sink.BeginWriting()?;
            Ok(CardWriter {
                sink,
                stream,
                device: device.clone(),
                context,
                video,
                video_device,
                processors,
                processor,
                allocator,
                inputs: Vec::new(),
                outputs: Vec::new(),
                upload: None,
                from,
                encoder,
            })
        }
    }

    /// The encoder that packs the video, as it names itself.
    pub fn encoder(&self) -> &str {
        &self.encoder
    }

    /// Adds a frame from a picture on the card, of the size given at the start. `time` is when it
    /// comes in the video and `length` how long it stays, both in Media Foundation's units. While
    /// the encoder is still busy with the frames before, it is left out, and it says so.
    pub fn write(&mut self, picture: &ID3D11Texture2D, time: i64, length: i64) -> Result<bool> {
        unsafe {
            let sample = match self.allocator.AllocateSample() {
                Ok(sample) => sample,
                Err(error) if error.code() == MF_E_SAMPLEALLOCATOR_EMPTY => return Ok(false),
                Err(error) => return Err(error),
            };
            let whole = sample.GetBufferByIndex(0)?;
            // The encoder takes a buffer to be empty until it is told how much it holds.
            whole.SetCurrentLength(whole.cast::<IMF2DBuffer>()?.GetContiguousLength()?)?;
            let buffer: IMFDXGIBuffer = whole.cast()?;
            let mut frame = null_mut();
            buffer.GetResource(&ID3D11Texture2D::IID, &mut frame)?;
            let frame = ID3D11Texture2D::from_raw(frame);
            let output = self.output_view(&frame, buffer.GetSubresourceIndex()?)?;
            let input = self.input_view(picture)?;
            let stream = D3D11_VIDEO_PROCESSOR_STREAM { Enable: true.into(), pInputSurface: ManuallyDrop::new(Some(input)), ..Default::default() };
            let turned = self.video.VideoProcessorBlt(&self.processor, &output, 0, std::slice::from_ref(&stream));
            drop(ManuallyDrop::into_inner(stream.pInputSurface));
            turned?;
            sample.SetSampleTime(time)?;
            sample.SetSampleDuration(length)?;
            self.sink.WriteSample(self.stream, &sample)?;
            Ok(true)
        }
    }

    /// Adds a frame from memory: rows from the top, four bytes a pixel, blue, green, red and one
    /// ignored, `stride` bytes from row to row, of the size given at the start.
    pub fn write_pixels(&mut self, pixels: &[u8], stride: usize, time: i64, length: i64) -> Result<bool> {
        let upload = match &self.upload {
            Some(upload) => upload.clone(),
            None => {
                let upload = picture(&self.device, self.from)?;
                self.upload = Some(upload.clone());
                upload
            }
        };
        if pixels.len() < stride * self.from.1 as usize {
            return Err(windows_core::Error::empty());
        }
        unsafe { self.context.UpdateSubresource(&upload, 0, None, pixels.as_ptr().cast(), stride as u32, 0) };
        self.write(&upload, time, length)
    }

    /// Ends the file.
    pub fn finish(&self) -> Result<()> {
        unsafe { self.sink.Finalize() }
    }

    fn input_view(&mut self, picture: &ID3D11Texture2D) -> Result<ID3D11VideoProcessorInputView> {
        let key = picture.as_raw() as usize;
        if let Some((_, view)) = self.inputs.iter().find(|(seen, _)| *seen == key) {
            return Ok(view.clone());
        }
        let description = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: 0 } },
        };
        let mut view = None;
        unsafe { self.video_device.CreateVideoProcessorInputView(picture, &self.processors, &description, Some(&mut view))? };
        let view = view.ok_or_else(windows_core::Error::empty)?;
        self.inputs.push((key, view.clone()));
        Ok(view)
    }

    fn output_view(&mut self, frame: &ID3D11Texture2D, slice: u32) -> Result<ID3D11VideoProcessorOutputView> {
        let key = (frame.as_raw() as usize, slice);
        if let Some((_, view)) = self.outputs.iter().find(|(seen, _)| *seen == key) {
            return Ok(view.clone());
        }
        let description = match slice {
            0 => D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 } },
            },
            // A frame in an array of textures.
            _ => D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2DARRAY,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2DArray: D3D11_TEX2D_ARRAY_VPOV { MipSlice: 0, FirstArraySlice: slice, ArraySize: 1 },
                },
            },
        };
        let mut view = None;
        unsafe { self.video_device.CreateVideoProcessorOutputView(frame, &self.processors, &description, Some(&mut view))? };
        let view = view.ok_or_else(windows_core::Error::empty)?;
        self.outputs.push((key, view.clone()));
        Ok(view)
    }
}

/// The encoder Media Foundation has put on a stream, by its name, if it is a graphics card's.
unsafe fn card_encoder(sink: &IMFSinkWriter, stream: u32) -> Option<String> {
    let sink: IMFSinkWriterEx = sink.cast().ok()?;
    for index in 0.. {
        let (mut category, mut transform) = (GUID::zeroed(), None);
        unsafe { sink.GetTransformForStream(stream, index, Some(&mut category), &mut transform) }.ok()?;
        if category != MFT_CATEGORY_VIDEO_ENCODER {
            continue;
        }
        let attributes = unsafe { transform?.GetAttributes() }.ok()?;
        // Only a card's encoder says where it is.
        text(&attributes, &MFT_ENUM_HARDWARE_URL_Attribute)?;
        let name = text(&attributes, &MFT_FRIENDLY_NAME_Attribute);
        return Some(name.or_else(|| text(&attributes, &MFT_ENUM_HARDWARE_VENDOR_ID_Attribute)).unwrap_or_default());
    }
    None
}

/// A text among attributes.
fn text(attributes: &IMFAttributes, key: &GUID) -> Option<String> {
    let (mut text, mut length) = (PWSTR::null(), 0);
    unsafe { attributes.GetAllocatedString(key, &mut text, &mut length) }.ok()?;
    let found = unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(text.0, length as usize)) };
    unsafe { CoTaskMemFree(Some(text.0 as *const _)) };
    Some(found)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    use super::*;
    use crate::win::video::{Reader, SECOND};

    /// A frame with a white square going across a dark ground, its top left corner red and its
    /// bottom left corner blue.
    fn frame(width: usize, height: usize, n: usize) -> Vec<u8> {
        let mut pixels = vec![40u8; width * height * 4];
        let at = n * 8 % (width - 64);
        let mut paint = |left: usize, top: usize, size: usize, colour: [u8; 3]| {
            for y in top..top + size {
                for x in left..left + size {
                    pixels[(y * width + x) * 4..(y * width + x) * 4 + 3].copy_from_slice(&colour);
                }
            }
        };
        paint(at, 32, 64, [250, 250, 250]);
        paint(0, 0, 24, [0, 0, 255]);
        paint(0, height - 24, 24, [255, 0, 0]);
        pixels
    }

    /// The processor's time the whole process has had, in seconds.
    fn processor_time() -> f64 {
        let (mut created, mut ended, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut ended, &mut kernel, &mut user) };
        let seconds = |time: FILETIME| ((time.dwHighDateTime as u64) << 32 | time.dwLowDateTime as u64) as f64 / 1e7;
        seconds(kernel) + seconds(user)
    }

    /// Frames of a size go through the card into a file and read back the right way up, with
    /// their colours; how fast, and how much of the processor it takes, is said on the way:
    /// `cargo test -p ainitshot frames_go_through_the_card -- --nocapture`. A computer without an
    /// encoder on its card has nothing to check.
    #[test]
    fn frames_go_through_the_card() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        let folder = std::env::temp_dir().join(format!("ainitshot-card-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let Ok((device, _)) = video_device(None) else { return };
        let count = std::env::var("CARD_FRAMES").ok().and_then(|n| n.parse().ok()).unwrap_or(90);
        for (width, height, rate) in [(642usize, 362usize, 30u32), (2018, 1276, 60)] {
            let path = folder.join(format!("{width}x{height}.mp4"));
            let writer = CardWriter::create(&path, &device, (width as u32, height as u32), (width as u32, height as u32), rate);
            let mut writer = match writer {
                Ok(writer) => writer,
                Err(error) => {
                    eprintln!("no encoder on the card: {error}");
                    return;
                }
            };
            // The pictures are on the card beforehand, as the screen's are.
            let pictures: Vec<ID3D11Texture2D> = (0..30)
                .map(|n| {
                    let pixels = frame(width, height, n);
                    let texture = picture(&device, (width as u32, height as u32)).unwrap();
                    let context = unsafe { device.GetImmediateContext().unwrap() };
                    unsafe { context.UpdateSubresource(&texture, 0, None, pixels.as_ptr().cast(), width as u32 * 4, 0) };
                    texture
                })
                .collect();
            let length = SECOND / rate as i64;
            let (start, used) = (Instant::now(), processor_time());
            let mut left_out = 0;
            for n in 0..count {
                if !writer.write(&pictures[n % 30], n as i64 * length, length).unwrap() {
                    left_out += 1;
                }
            }
            writer.finish().unwrap();
            let (took, used) = (start.elapsed(), processor_time() - used);
            let bytes = std::fs::metadata(&path).unwrap().len();
            eprintln!(
                "{width}x{height} by {}: {count} frames in {took:?}, {:.1} ms of the processor a frame, {left_out} left out, {} KB",
                writer.encoder(),
                used * 1000.0 / count as f64,
                bytes / 1024
            );
            let mut reader = Reader::open(&path).unwrap();
            assert_eq!(reader.rate, rate);
            let mut read = 0;
            while let Some((_, pixels)) = reader.next_frame().unwrap() {
                assert_eq!((reader.width, reader.height), (width, height));
                let at = |x: usize, y: usize| [pixels[(y * width + x) * 4], pixels[(y * width + x) * 4 + 1], pixels[(y * width + x) * 4 + 2]];
                let near = |got: [u8; 3], wanted: [u8; 3]| got.iter().zip(wanted).all(|(&a, b)| (a as i32 - b as i32).abs() <= 8);
                if read == 0 {
                    eprintln!("  red {:?}, blue {:?}, ground {:?}", at(8, 8), at(8, height - 8), at(width - 8, height / 2));
                }
                assert!(near(at(8, 8), [0, 0, 255]), "{:?}", at(8, 8));
                assert!(near(at(8, height - 8), [255, 0, 0]), "{:?}", at(8, height - 8));
                assert!(near(at(width - 8, height / 2), [40, 40, 40]), "{:?}", at(width - 8, height / 2));
                read += 1;
            }
            assert!(read + 2 >= count - left_out, "{read} frames of {count}");
        }
        if std::env::var_os("CARD_KEEP").is_none() {
            std::fs::remove_dir_all(&folder).ok();
        }
    }

    /// A picture of a real screen through the card: two seconds of it going up and three of a
    /// small square moving on it, how near to it the frames come out and how large the file is,
    /// beside the processor's encoder:
    /// `SCREENSHOT=<file.png> cargo test -p ainitshot a_screen_goes_through_the_card -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_screen_goes_through_the_card() {
        use crate::geometry::Rect;
        use crate::image::Image;
        use crate::win::video::Writer;
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        let file = std::env::var_os("SCREENSHOT").unwrap();
        let picture_of = Image::from_png(&std::fs::read(file).unwrap()).unwrap();
        let (width, height) = (picture_of.width as usize & !1, (picture_of.height as usize & !1) - 200);
        let folder = std::env::temp_dir().join(format!("ainitshot-card-screen-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        // The frames a second of a recording by default, unless asked for others.
        let fps = std::env::var("CARD_RATE").ok().and_then(|rate| rate.parse().ok()).unwrap_or(60u32);
        let length = SECOND / fps as i64;
        let count = 5 * fps as usize;
        // The same motion a second whatever the frames a second: the page goes up 100 pixels a
        // second for two seconds, the square 210 pixels a second to the right and 90 down.
        let rate = fps as usize;
        let shown = |n: usize| {
            let top = (n * 100 / rate).min(200) as i32;
            let mut pixels = picture_of.crop(Rect::new(0, top, width as i32, top + height as i32)).pixels;
            let (x, y) = (100 + n * 210 / rate, 100 + n * 90 / rate);
            for row in y..y + 16 {
                pixels[(row * width + x) * 4..(row * width + x + 12) * 4].fill(0);
            }
            pixels
        };
        let quality = |path: &Path| {
            let mut reader = Reader::open(path).unwrap();
            let (mut n, mut quality) = (0, Vec::new());
            while let Some((_, pixels)) = reader.next_frame().unwrap() {
                if n == 30 || n == 100 {
                    let source = shown(n);
                    let sum: f64 = pixels.iter().zip(&source).enumerate().filter(|(at, _)| at % 4 != 3).map(|(_, (&a, &b))| (a as f64 - b as f64).powi(2)).sum();
                    quality.push((10.0 * (255.0f64 * 255.0 / (sum / (pixels.len() as f64 * 0.75))).log10() * 10.0).round() / 10.0);
                }
                n += 1;
            }
            (n, quality)
        };
        let size = (width as u32, height as u32);
        let (device, context) = video_device(None).unwrap();
        let pictures: Vec<ID3D11Texture2D> = (0..count)
            .map(|n| {
                let texture = picture(&device, size).unwrap();
                unsafe { context.UpdateSubresource(&texture, 0, None, shown(n).as_ptr().cast(), width as u32 * 4, 0) };
                texture
            })
            .collect();
        let path = folder.join("card.mp4");
        let mut writer = CardWriter::create(&path, &device, size, size, fps).unwrap();
        let (start, used) = (Instant::now(), processor_time());
        for (n, picture) in pictures.iter().enumerate() {
            assert!(writer.write(picture, n as i64 * length, length).unwrap());
        }
        writer.finish().unwrap();
        let (took, used) = (start.elapsed(), processor_time() - used);
        let encoder = writer.encoder().to_string();
        let (frames, decibels) = quality(&path);
        let kilobytes = |path: &Path| std::fs::metadata(path).unwrap().len() / 1024;
        eprintln!("{width}x{height}, the card ({encoder}): {took:?}, {:.1} ms of the processor a frame, {} KB, {frames} frames at {decibels:?} dB", used * 1000.0 / count as f64, kilobytes(&path));
        let path = folder.join("processor.mp4");
        let frames: Vec<Vec<u8>> = (0..count).map(shown).collect();
        let mut writer = Writer::create(&path, size.0, size.1, fps, None).unwrap();
        let (start, used) = (Instant::now(), processor_time());
        for (n, pixels) in frames.iter().enumerate() {
            writer.write(pixels, width * 4, n as i64 * length, length).unwrap();
        }
        writer.finish().unwrap();
        let (took, used) = (start.elapsed(), processor_time() - used);
        let (frames, decibels) = quality(&path);
        eprintln!("{width}x{height}, the processor: {took:?}, {:.1} ms of the processor a frame, {} KB, {frames} frames at {decibels:?} dB", used * 1000.0 / count as f64, kilobytes(&path));
        if std::env::var_os("CARD_KEEP").is_none() {
            std::fs::remove_dir_all(&folder).ok();
        }
    }
}
