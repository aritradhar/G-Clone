//! The G-Clone logo: a teal "G" with a skull on its lower right, rasterized in code
//! (no image files) at any size with 4x4 supersampling.

const TEAL: [u8; 3] = [0x00, 0xC8, 0xD7];
const BONE: [u8; 3] = [0xEE, 0xEE, 0xEA];
const OUTLINE: [u8; 3] = [0x10, 0x12, 0x15];

/// Colour at normalized position (u, v) in [0, 1], or `None` for transparent.
fn sample(u: f32, v: f32) -> Option<[u8; 3]> {
    if in_skull(u, v, 0.0) {
        return Some(BONE);
    }
    if in_skull(u, v, 0.035) {
        return Some(OUTLINE);
    }
    in_g(u, v).then_some(TEAL)
}

fn in_g(u: f32, v: f32) -> bool {
    let (cx, cy) = (0.42, 0.46);
    let (dx, dy) = (u - cx, v - cy);
    let r = (dx * dx + dy * dy).sqrt();
    let angle = dy.atan2(dx);
    let ring = (0.27..=0.40).contains(&r) && !(-0.95..0.05).contains(&angle);
    let bar = (0.02..=0.40).contains(&dx) && (-0.01..=0.10).contains(&dy) && r <= 0.40;
    ring || bar
}

/// Skull centred lower right; `grow` expands the silhouette (for the outline).
fn in_skull(u: f32, v: f32, grow: f32) -> bool {
    let (cx, cy, r) = (0.74, 0.66, 0.19 + grow);
    let (dx, dy) = (u - cx, v - cy);
    let cranium = dx * dx + dy * dy <= r * r;
    let jaw = (cx - 0.12 - grow..=cx + 0.12 + grow).contains(&u) && (cy..=cy + 0.28 + grow).contains(&v);
    if !(cranium || jaw) {
        return false;
    }
    if grow > 0.0 {
        return true;
    }
    // Holes: eyes, nose, gaps between teeth.
    let eye = |ex: f32| {
        let (a, b) = (u - ex, v - (cy + 0.02));
        a * a / (0.052 * 0.052) + b * b / (0.06 * 0.06) <= 1.0
    };
    let nose = {
        let t = (v - (cy + 0.10)) / 0.07;
        (0.0..=1.0).contains(&t) && (u - cx).abs() <= 0.025 * (1.0 - t) + 0.008
    };
    let teeth = v >= cy + 0.21 && [-0.06, 0.0, 0.06].iter().any(|o| (u - (cx + o)).abs() <= 0.012);
    !(eye(cx - 0.075) || eye(cx + 0.075) || nose || teeth)
}

/// RGBA pixels of the logo at `size` x `size`.
pub fn rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 4;
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (mut rgb, mut hits) = ([0u32; 3], 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    if let Some(c) = sample(u, v) {
                        for i in 0..3 {
                            rgb[i] += c[i] as u32;
                        }
                        hits += 1;
                    }
                }
            }
            if hits > 0 {
                let i = ((y * size + x) * 4) as usize;
                for c in 0..3 {
                    px[i + c] = (rgb[c] / hits) as u8;
                }
                px[i + 3] = (hits * 255 / (SS * SS)) as u8;
            }
        }
    }
    px
}
