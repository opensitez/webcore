use tiny_skia::Pixmap;

pub(super) fn filter_pixmap(input: &Pixmap, rx: usize, ry: usize, dilate: bool) -> Pixmap {
    if rx == 0 && ry == 0 {
        return input.clone();
    }
    let width = input.width() as usize;
    let height = input.height() as usize;
    let kernel_width = rx.saturating_mul(2).saturating_add(1).min(width);
    let kernel_height = ry.saturating_mul(2).saturating_add(1).min(height);
    if kernel_width.saturating_mul(kernel_height) <= 9 {
        return small_window_filter(input, rx, ry, dilate);
    }
    let mut scratch = vec![0; input.data().len()];
    let mut output = Pixmap::new(input.width(), input.height()).expect("valid filter dimensions");
    let mut queue = vec![0; width.max(height)];
    extrema_pass(
        input.data(),
        &mut scratch,
        width,
        height,
        rx,
        true,
        dilate,
        &mut queue,
    );
    extrema_pass(
        &scratch,
        output.data_mut(),
        width,
        height,
        ry,
        false,
        dilate,
        &mut queue,
    );
    output
}

fn small_window_filter(input: &Pixmap, rx: usize, ry: usize, dilate: bool) -> Pixmap {
    let (width, height) = (input.width() as usize, input.height() as usize);
    let mut output = Pixmap::new(input.width(), input.height()).expect("valid filter dimensions");
    for y in 0..height {
        for x in 0..width {
            let mut channels = [if dilate { 0u8 } else { 255u8 }; 4];
            for sy in y.saturating_sub(ry)..=y.saturating_add(ry).min(height - 1) {
                for sx in x.saturating_sub(rx)..=x.saturating_add(rx).min(width - 1) {
                    let offset = (sy * width + sx) * 4;
                    for channel in 0..4 {
                        let value = input.data()[offset + channel];
                        channels[channel] = if dilate {
                            channels[channel].max(value)
                        } else {
                            channels[channel].min(value)
                        };
                    }
                }
            }
            let offset = (y * width + x) * 4;
            output.data_mut()[offset..offset + 4].copy_from_slice(&channels);
        }
    }
    output
}

// Rectangular channel extrema are separable. The monotonic queue visits each
// sample once and retains the existing clipped-window edge behavior.
fn extrema_pass(
    input: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
    radius: usize,
    horizontal: bool,
    dilate: bool,
    queue: &mut [usize],
) {
    if radius == 0 {
        output.copy_from_slice(input);
        return;
    }
    let (length, lines, step) = if horizontal {
        (width, height, 4)
    } else {
        (height, width, width * 4)
    };
    let radius = radius.min(length - 1);
    for line in 0..lines {
        let base = if horizontal {
            line * width * 4
        } else {
            line * 4
        };
        for channel in 0..4 {
            let base = base + channel;
            let (mut head, mut tail, mut next) = (0, 0, 0);
            for center in 0..length {
                let end = center.saturating_add(radius).min(length - 1);
                while next <= end {
                    let value = input[base + next * step];
                    while tail > head {
                        let previous = input[base + queue[tail - 1] * step];
                        if if dilate {
                            previous > value
                        } else {
                            previous < value
                        } {
                            break;
                        }
                        tail -= 1;
                    }
                    queue[tail] = next;
                    tail += 1;
                    next += 1;
                }
                let start = center.saturating_sub(radius);
                while queue[head] < start {
                    head += 1;
                }
                output[base + center * step] = input[base + queue[head] * step];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar(input: &Pixmap, rx: usize, ry: usize, dilate: bool) -> Pixmap {
        let (width, height) = (input.width() as usize, input.height() as usize);
        let mut output = Pixmap::new(input.width(), input.height()).unwrap();
        for y in 0..height {
            for x in 0..width {
                let mut channels = [if dilate { 0 } else { 255 }; 4];
                for sy in y.saturating_sub(ry)..=y.saturating_add(ry).min(height - 1) {
                    for sx in x.saturating_sub(rx)..=x.saturating_add(rx).min(width - 1) {
                        let offset = (sy * width + sx) * 4;
                        for channel in 0..4 {
                            let value = input.data()[offset + channel];
                            channels[channel] = if dilate {
                                channels[channel].max(value)
                            } else {
                                channels[channel].min(value)
                            };
                        }
                    }
                }
                for channel in 0..3 {
                    channels[channel] = channels[channel].min(channels[3]);
                }
                output.data_mut()[(y * width + x) * 4..(y * width + x + 1) * 4]
                    .copy_from_slice(&channels);
            }
        }
        output
    }

    fn fixture(width: u32, height: u32) -> Pixmap {
        let mut image = Pixmap::new(width, height).unwrap();
        let mut seed = 0x4a31u32;
        for pixel in image.data_mut().chunks_exact_mut(4) {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let alpha = (seed >> 24) as u8;
            pixel.copy_from_slice(&[
                (seed as u8).min(alpha),
                ((seed >> 8) as u8).min(alpha),
                ((seed >> 16) as u8).min(alpha),
                alpha,
            ]);
        }
        image
    }

    #[test]
    fn sliding_extrema_match_scalar_channels_edges_and_large_radii() {
        for (width, height) in [(1, 1), (1, 17), (19, 1), (7, 9), (32, 23)] {
            let input = fixture(width, height);
            for (rx, ry) in [
                (0, 0),
                (0, 3),
                (4, 0),
                (1, 1),
                (3, 5),
                (31, 27),
                (usize::MAX, usize::MAX),
            ] {
                for dilate in [false, true] {
                    let actual = filter_pixmap(&input, rx, ry, dilate);
                    let expected = scalar(&input, rx, ry, dilate);
                    assert_eq!(
                        actual.data(),
                        expected.data(),
                        "{width}x{height} radius={rx},{ry} dilate={dilate}"
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "manual release timing; exact-output check precedes timing"]
    fn morphology_sliding_window_benchmark() {
        use std::{hint::black_box, time::Instant};
        let input = fixture(512, 512);
        for radius in [1, 4, 12] {
            for dilate in [false, true] {
                assert_eq!(
                    filter_pixmap(&input, radius, radius, dilate).data(),
                    scalar(&input, radius, radius, dilate).data()
                );
                let mut fast = Vec::new();
                let mut slow = Vec::new();
                for iteration in 0..6 {
                    for optimized in if iteration % 2 == 0 {
                        [true, false]
                    } else {
                        [false, true]
                    } {
                        let start = Instant::now();
                        let result = if optimized {
                            filter_pixmap(black_box(&input), radius, radius, dilate)
                        } else {
                            scalar(black_box(&input), radius, radius, dilate)
                        };
                        black_box(result);
                        if optimized {
                            fast.push(start.elapsed());
                        } else {
                            slow.push(start.elapsed());
                        }
                    }
                }
                fast.sort();
                slow.sort();
                eprintln!(
                    "morphology 512x512 radius={radius} dilate={dilate}: sliding={:?} scalar={:?}",
                    fast[fast.len() / 2],
                    slow[slow.len() / 2]
                );
            }
        }
    }
}
