//! `agent-sim encode-bench`: times color conversion and H.264 encoding on synthetic frames that scroll like a list,
//! so the streaming path can be tuned without a phone.

use crate::video::{VideoEncoder, bgra_to_i420};
use anyhow::Result;
use openh264::formats::YUVSource;
use std::time::Instant;

/// A list-like screen: rows of light cells with dark "text" bars, on a white background.
fn base_frame(width: usize, height: usize) -> Vec<u8> {
    let mut px = vec![255u8; width * height * 4];
    for y in 0..height {
        let row = y / 44;
        let in_row = y % 44;
        for x in 0..width {
            let p = (y * width + x) * 4;
            if in_row < 2 {
                px[p..p + 3].copy_from_slice(&[220, 220, 220]);
            } else if (14..30).contains(&in_row) && x > 40 && x < 40 + 200 + (row * 37) % 300 {
                let shade = 30 + ((x / 7 + row) % 3) as u8 * 20;
                px[p..p + 3].copy_from_slice(&[shade, shade, shade]);
            } else if x < 32 && (10..34).contains(&in_row) {
                px[p..p + 3].copy_from_slice(&[(row * 50 % 256) as u8, 120, 200]);
            }
        }
    }
    px
}

fn stats(mut v: Vec<f64>) -> (f64, f64) {
    v.sort_by(|a, b| a.total_cmp(b));
    let avg = v.iter().sum::<f64>() / v.len() as f64;
    (avg, v[(v.len() * 95 / 100).min(v.len() - 1)])
}

pub fn run(
    width: usize,
    height: usize,
    scale: usize,
    frames: usize,
    bitrate_bps: u32,
) -> Result<()> {
    let base = base_frame(width, height * 2);
    let mut encoder = VideoEncoder::new(bitrate_bps)?;
    let mut decoder = openh264::decoder::Decoder::new()?;
    let (mut convert, mut encode, mut psnr, mut bytes) =
        (Vec::new(), Vec::new(), Vec::new(), 0usize);
    for i in 0..frames {
        // scroll by 24 px a frame through a page twice the screen's height
        let off = (i * 24) % height;
        let frame = &base[off * width * 4..(off + height) * width * 4];
        let t0 = Instant::now();
        let yuv = bgra_to_i420(frame, width, height, scale);
        let t1 = Instant::now();
        let out = encoder.encode(&yuv)?;
        let t2 = Instant::now();
        bytes += out.annexb.len();
        convert.push((t1 - t0).as_secs_f64() * 1e3);
        encode.push((t2 - t1).as_secs_f64() * 1e3);
        // quality: decode the frame and compare its luma with what went in
        if let Some(dec) = decoder.decode(&out.annexb)? {
            let (w, h) = dec.dimensions();
            let stride = dec.strides().0;
            let (mut se, mut n) = (0f64, 0f64);
            for row in 0..h.min(yuv.height) {
                let a = &dec.y()[row * stride..row * stride + w];
                let b = &yuv.y_plane()[row * yuv.width..row * yuv.width + w];
                for (p, q) in a.iter().zip(b) {
                    let d = *p as f64 - *q as f64;
                    se += d * d;
                    n += 1.0;
                }
            }
            let mse = (se / n).max(1e-9);
            psnr.push(10.0 * (255.0f64 * 255.0 / mse).log10());
        }
    }
    let (ca, cp) = stats(convert);
    let (ea, ep) = stats(encode);
    let quality = psnr.iter().sum::<f64>() / psnr.len().max(1) as f64;
    println!(
        "{width}x{height} at 1/{scale}: convert {ca:.2} ms (p95 {cp:.2}), encode {ea:.2} ms (p95 {ep:.2}), \
         {:.0} kbit/s at 60 fps, luma PSNR {quality:.1} dB",
        bytes as f64 * 8.0 / frames as f64 * 60.0 / 1000.0
    );
    Ok(())
}
