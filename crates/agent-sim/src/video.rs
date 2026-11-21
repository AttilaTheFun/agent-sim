//! BGRA frames to H.264, one frame in and one access unit out, with no buffering in between.

use anyhow::{Context, Result};
use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Complexity, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, Profile,
    RateControlMode, SpsPpsStrategy, UsageType,
};
use openh264::formats::YUVSlices;

/// I420 planes of a frame scaled down by an integer factor.
pub struct Yuv {
    pub width: usize,
    pub height: usize,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

/// BT.601 limited range, from 8-bit sums of `n` pixels.
fn luma(r: u32, g: u32, b: u32, n: u32) -> u8 {
    (((66 * r + 129 * g + 25 * b) / n + 128) >> 8) as u8 + 16
}
fn chroma(r: u32, g: u32, b: u32, n: u32) -> (u8, u8) {
    let (r, g, b) = ((r / n) as i32, (g / n) as i32, (b / n) as i32);
    let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
    let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
    (u.clamp(0, 255) as u8, v.clamp(0, 255) as u8)
}

/// Converts packed BGRA to I420, averaging `scale` x `scale` blocks; output sizes are rounded down to even.
pub fn bgra_to_i420(bgra: &[u8], width: usize, height: usize, scale: usize) -> Yuv {
    let scale = scale.max(1);
    let ow = (width / scale) & !1;
    let oh = (height / scale) & !1;
    let mut y = vec![0u8; ow * oh];
    let mut u = vec![0u8; ow / 2 * oh / 2];
    let mut v = vec![0u8; ow / 2 * oh / 2];
    let n = (scale * scale) as u32;
    for by in (0..oh).step_by(2) {
        for bx in (0..ow).step_by(2) {
            let (mut cr, mut cg, mut cb) = (0u32, 0u32, 0u32);
            for dy in 0..2 {
                for dx in 0..2 {
                    let (oy, ox) = (by + dy, bx + dx);
                    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                    for sy in 0..scale {
                        let row = ((oy * scale + sy) * width + ox * scale) * 4;
                        for sx in 0..scale {
                            let p = row + sx * 4;
                            b += bgra[p] as u32;
                            g += bgra[p + 1] as u32;
                            r += bgra[p + 2] as u32;
                        }
                    }
                    y[oy * ow + ox] = luma(r, g, b, n);
                    cr += r;
                    cg += g;
                    cb += b;
                }
            }
            let (cu, cv) = chroma(cr, cg, cb, n * 4);
            let i = by / 2 * (ow / 2) + bx / 2;
            u[i] = cu;
            v[i] = cv;
        }
    }
    Yuv {
        width: ow,
        height: oh,
        y,
        u,
        v,
    }
}

pub struct Encoded {
    pub keyframe: bool,
    pub annexb: Vec<u8>,
}

/// An OpenH264 encoder tuned for screen content and low latency.
pub struct VideoEncoder {
    encoder: Encoder,
    size: (usize, usize),
}

impl VideoEncoder {
    pub fn new(bitrate_bps: u32) -> Result<Self> {
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .profile(Profile::Baseline)
            .complexity(Complexity::Low)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate_bps))
            .max_frame_rate(FrameRate::from_hz(60.0))
            .skip_frames(false)
            .sps_pps_strategy(SpsPpsStrategy::ConstantId)
            .intra_frame_period(IntraFramePeriod::from_num_frames(0));
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config)
            .context("starting the H.264 encoder")?;
        Ok(Self {
            encoder,
            size: (0, 0),
        })
    }

    /// The next frame is a keyframe (a new viewer, or a decoder that lost its place).
    pub fn force_keyframe(&mut self) {
        self.encoder.force_intra_frame();
    }

    pub fn encode(&mut self, yuv: &Yuv) -> Result<Encoded> {
        if self.size != (yuv.width, yuv.height) {
            self.size = (yuv.width, yuv.height);
            self.encoder.force_intra_frame();
        }
        let slices = YUVSlices::new(
            (&yuv.y, &yuv.u, &yuv.v),
            (yuv.width, yuv.height),
            (yuv.width, yuv.width / 2, yuv.width / 2),
        );
        let stream = self.encoder.encode(&slices).context("encoding a frame")?;
        Ok(Encoded {
            keyframe: matches!(stream.frame_type(), FrameType::IDR | FrameType::I),
            annexb: stream.to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_and_black_map_to_limited_range() {
        let white = vec![255u8; 4 * 4 * 4];
        let yuv = bgra_to_i420(&white, 4, 4, 2);
        assert_eq!((yuv.width, yuv.height), (2, 2));
        assert_eq!(yuv.y[0], 235);
        assert_eq!((yuv.u[0], yuv.v[0]), (128, 128));
        let black = vec![0u8; 4 * 4 * 4];
        assert_eq!(bgra_to_i420(&black, 4, 4, 2).y[0], 16);
    }

    #[test]
    fn encodes_a_keyframe_first() {
        let frame = vec![128u8; 64 * 64 * 4];
        let yuv = bgra_to_i420(&frame, 64, 64, 1);
        let mut enc = VideoEncoder::new(1_000_000).unwrap();
        let out = enc.encode(&yuv).unwrap();
        assert!(out.keyframe);
        assert!(out.annexb.starts_with(&[0, 0, 0, 1]));
    }
}
