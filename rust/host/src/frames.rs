//! Screen frames and the changes sent to the viewer.
//!
//! A [`FrameSource`] captures the whole screen (Windows: the real desktop;
//! tests: a synthetic picture). [`Differ`] compares each new frame with the
//! previous one in 64×64 tiles and returns only the changed rectangles, so a
//! still screen costs almost nothing to share.

use core::num::{NonZeroU16, NonZeroUsize};

use bytes::Bytes;
use ironrdp_server::{BitmapUpdate, PixelFormat};

/// One captured screen picture: 4 bytes per pixel in BGRA (or BGRX) order.
#[derive(Clone)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub bgra: Vec<u8>,
}

impl Frame {
    fn row(&self, y: usize) -> &[u8] {
        let w = usize::from(self.width) * 4;
        &self.bgra[y * w..(y + 1) * w]
    }
}

/// Something that can capture the screen.
pub trait FrameSource: Send {
    /// Size of the screen in pixels.
    fn size(&mut self) -> (u16, u16);
    /// Captures the screen now.
    fn capture(&mut self) -> anyhow::Result<Frame>;
}

const TILE: usize = 64;

/// Finds what changed between frames.
#[derive(Default)]
pub struct Differ {
    previous: Option<Frame>,
}

impl Differ {
    /// Changed areas of `frame` (everything for the first frame or after a size change).
    pub fn changes(&mut self, frame: Frame) -> Vec<BitmapUpdate> {
        let full = match &self.previous {
            Some(p) => p.width != frame.width || p.height != frame.height,
            None => true,
        };
        let rects = if full {
            vec![(0, 0, usize::from(frame.width), usize::from(frame.height))]
        } else {
            self.changed_tiles(&frame)
        };
        let updates = rects
            .into_iter()
            .filter_map(|(x, y, w, h)| region(&frame, x, y, w, h))
            .collect();
        self.previous = Some(frame);
        updates
    }

    /// Changed tiles, merged into one rectangle per row of tiles.
    fn changed_tiles(&self, frame: &Frame) -> Vec<(usize, usize, usize, usize)> {
        let Some(prev) = &self.previous else {
            return Vec::new();
        };
        let (w, h) = (usize::from(frame.width), usize::from(frame.height));
        let mut out = Vec::new();
        for ty in (0..h).step_by(TILE) {
            let th = TILE.min(h - ty);
            let mut run: Option<(usize, usize)> = None; // (first x, last x end)
            for tx in (0..w).step_by(TILE) {
                let tw = TILE.min(w - tx);
                let changed = (ty..ty + th).any(|y| {
                    let a = &prev.row(y)[tx * 4..(tx + tw) * 4];
                    let b = &frame.row(y)[tx * 4..(tx + tw) * 4];
                    a != b
                });
                run = match (run, changed) {
                    (None, true) => Some((tx, tx + tw)),
                    (Some((start, _)), true) => Some((start, tx + tw)),
                    (Some((start, end)), false) => {
                        out.push((start, ty, end - start, th));
                        None
                    }
                    (None, false) => None,
                };
            }
            if let Some((start, end)) = run {
                out.push((start, ty, end - start, th));
            }
        }
        out
    }
}

/// Copies one rectangle of the frame into an update.
fn region(frame: &Frame, x: usize, y: usize, w: usize, h: usize) -> Option<BitmapUpdate> {
    if w == 0 || h == 0 {
        return None;
    }
    let mut data = Vec::with_capacity(w * h * 4);
    for row in y..y + h {
        data.extend_from_slice(&frame.row(row)[x * 4..(x + w) * 4]);
    }
    Some(BitmapUpdate {
        x: u16::try_from(x).ok()?,
        y: u16::try_from(y).ok()?,
        width: NonZeroU16::new(u16::try_from(w).ok()?)?,
        height: NonZeroU16::new(u16::try_from(h).ok()?)?,
        format: PixelFormat::BgrX32,
        data: Bytes::from(data),
        stride: NonZeroUsize::new(w * 4)?,
    })
}

/// A moving test picture, for tests and for trying the Host without a real screen.
pub struct SyntheticSource {
    pub width: u16,
    pub height: u16,
    tick: u32,
}

impl SyntheticSource {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            tick: 0,
        }
    }
}

impl FrameSource for SyntheticSource {
    fn size(&mut self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn capture(&mut self) -> anyhow::Result<Frame> {
        let (w, h) = (usize::from(self.width), usize::from(self.height));
        let mut bgra = vec![0x30u8; w * h * 4];
        // A small square that moves one tile every capture.
        let pos = (self.tick as usize * TILE) % w.max(1);
        for y in 0..TILE.min(h) {
            for x in pos..(pos + TILE).min(w) {
                let i = (y * w + x) * 4;
                bgra[i..i + 4].copy_from_slice(&[0x40, 0xC0, 0x20, 0xFF]);
            }
        }
        self.tick = self.tick.wrapping_add(1);
        Ok(Frame {
            width: self.width,
            height: self.height,
            bgra,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u16, h: u16, v: u8) -> Frame {
        Frame {
            width: w,
            height: h,
            bgra: vec![v; usize::from(w) * usize::from(h) * 4],
        }
    }

    #[test]
    fn first_frame_is_sent_whole_then_only_changes() {
        let mut d = Differ::default();
        let first = d.changes(solid(200, 100, 1));
        assert_eq!(first.len(), 1);
        assert_eq!((first[0].width.get(), first[0].height.get()), (200, 100));

        // Nothing changed: nothing sent.
        assert!(d.changes(solid(200, 100, 1)).is_empty());

        // One pixel changes in the second tile of the second tile row.
        let mut f = solid(200, 100, 1);
        let i = (70 * 200 + 70) * 4;
        f.bgra[i] = 9;
        let u = d.changes(f);
        assert_eq!(u.len(), 1);
        assert_eq!((u[0].x, u[0].y), (64, 64));
        assert_eq!((u[0].width.get(), u[0].height.get()), (64, 36));
        assert_eq!(u[0].data.len(), 64 * 36 * 4);
    }

    #[test]
    fn size_change_sends_everything() {
        let mut d = Differ::default();
        let _ = d.changes(solid(100, 100, 1));
        let u = d.changes(solid(120, 100, 1));
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].width.get(), 120);
    }

    #[test]
    fn synthetic_source_moves() -> anyhow::Result<()> {
        let mut s = SyntheticSource::new(256, 128);
        let mut d = Differ::default();
        let _ = d.changes(s.capture()?);
        assert!(!d.changes(s.capture()?).is_empty());
        Ok(())
    }
}
