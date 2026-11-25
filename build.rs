#[cfg(windows)]
fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)).round() as u8
}

#[cfg(windows)]
fn pink_white_gradient(t: f32) -> [u8; 4] {
    // Diagonal Sakura Pink (#FF4D94) -> Soft Blush (#FFA6D2) -> Pearl White (#FFF5FA)
    let t = t.clamp(0.0, 1.0);
    if t < 0.55 {
        let k = t / 0.55;
        [lerp_u8(255, 255, k), lerp_u8(77, 166, k), lerp_u8(148, 210, k), 255]
    } else {
        let k = (t - 0.55) / 0.45;
        [lerp_u8(255, 255, k), lerp_u8(166, 245, k), lerp_u8(210, 250, k), 255]
    }
}

#[cfg(windows)]
fn dist_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len_sq = abx * abx + aby * aby;
    if len_sq <= 1e-8 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = (((px - ax) * abx + (py - ay) * aby) / len_sq).clamp(0.0, 1.0);
    let cx = ax + t * abx;
    let cy = ay + t * aby;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// Signed distance to a centered rounded box spanning [pad, 1.0 - pad] with corner radius `r`.
#[cfg(windows)]
fn rounded_box_sdf(u: f32, v: f32, half: f32, radius: f32) -> f32 {
    let qx = (u - 0.5).abs() - (half - radius);
    let qy = (v - 0.5).abs() - (half - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    outside + inside - radius
}

/// Samples a single normalized (u, v) point of the unified AMOLED rounded-square brand emblem
/// (matches paint_brand_mark in src/main.rs and RadarIcon.tsx).
#[cfg(windows)]
fn sample_icon_rgba(u: f32, v: f32) -> [u8; 4] {
    let outer_half = 0.47;
    let radius = 0.16;
    let border_w = 0.068;
    let sdf = rounded_box_sdf(u, v, outer_half, radius);

    if sdf > 0.0 {
        return [0, 0, 0, 0];
    }

    let in_border = sdf >= -border_w;
    // Symmetrical centered V matching the in-app brand mark
    let d_left = dist_to_segment(u, v, 0.27, 0.27, 0.50, 0.74);
    let d_right = dist_to_segment(u, v, 0.50, 0.74, 0.73, 0.27);
    let in_v = d_left.min(d_right) <= 0.058;

    // Signature pink-to-pearl radar dot nested cleanly above the V cleft
    let dot_dist = ((u - 0.50).powi(2) + (v - 0.30).powi(2)).sqrt();
    let in_dot = dot_dist <= 0.072;

    if in_border {
        let grad_t = u * 0.55 + (1.0 - v) * 0.45;
        pink_white_gradient(grad_t)
    } else if in_dot {
        [255, 92, 168, 255]
    } else if in_v {
        // Crisp Pearl White V matching the in-app title bar emblem
        let grad_t = 0.78 + (1.0 - v) * 0.22;
        pink_white_gradient(grad_t)
    } else {
        // Pure AMOLED Black with subtle midnight depth (#000000 -> #0A080E)
        [lerp_u8(0, 10, v), lerp_u8(0, 8, v), lerp_u8(0, 14, v), 255]
    }
}

#[cfg(windows)]
fn render_icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    // 2x2 supersampling for crisp anti-aliased rounded corners and V edges across all ICO sizes
    let offsets = [0.25f32, 0.75f32];
    for y in 0..size {
        for x in 0..size {
            let mut r_acc = 0.0f32;
            let mut g_acc = 0.0f32;
            let mut b_acc = 0.0f32;
            let mut a_acc = 0.0f32;
            for &oy in &offsets {
                for &ox in &offsets {
                    let u = (x as f32 + ox) / size as f32;
                    let v = (y as f32 + oy) / size as f32;
                    let s = sample_icon_rgba(u, v);
                    let a = s[3] as f32 / 255.0;
                    r_acc += s[0] as f32 * a;
                    g_acc += s[1] as f32 * a;
                    b_acc += s[2] as f32 * a;
                    a_acc += a;
                }
            }
            let idx = ((y * size + x) * 4) as usize;
            let avg_a = a_acc / 4.0;
            if avg_a <= 0.001 {
                rgba[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
            } else {
                rgba[idx] = (r_acc / a_acc).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 1] = (g_acc / a_acc).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 2] = (b_acc / a_acc).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 3] = (avg_a * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    rgba
}

#[cfg(windows)]
fn build_dib_entry(size: u32) -> Vec<u8> {
    let rgba = render_icon_rgba(size);
    let xor_len = size * size * 4;
    let and_row_bytes = ((size + 31) / 32) * 4;
    let and_len = and_row_bytes * size;

    let mut dib = Vec::with_capacity((40 + xor_len + and_len) as usize);
    // BITMAPINFOHEADER (40 bytes)
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&(size as i32).to_le_bytes());
    dib.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // height * 2 (XOR + AND)
    dib.extend_from_slice(&1u16.to_le_bytes()); // planes
    dib.extend_from_slice(&32u16.to_le_bytes()); // 32-bit BGRA
    dib.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    dib.extend_from_slice(&(xor_len + and_len).to_le_bytes());
    dib.extend_from_slice(&[0u8; 16]); // ppm + color counts

    // Bottom-up BGRA pixels
    for y in (0..size).rev() {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            dib.push(rgba[idx + 2]); // B
            dib.push(rgba[idx + 1]); // G
            dib.push(rgba[idx]); // R
            dib.push(rgba[idx + 3]); // A
        }
    }

    // 1bpp AND mask matching transparent rounded corners (bit = 1 where alpha == 0)
    for y in (0..size).rev() {
        let mut row = vec![0u8; and_row_bytes as usize];
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            if rgba[idx + 3] == 0 {
                row[(x / 8) as usize] |= 1u8 << (7 - (x % 8));
            }
        }
        dib.extend_from_slice(&row);
    }
    dib
}

#[cfg(windows)]
fn build_multi_res_ico(sizes: &[u32]) -> Vec<u8> {
    let entries: Vec<(u32, Vec<u8>)> = sizes.iter().map(|&s| (s, build_dib_entry(s))).collect();
    let mut ico = Vec::new();

    // ICONDIR (6 bytes)
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type = ICO
    ico.extend_from_slice(&(entries.len() as u16).to_le_bytes());

    // ICONDIRENTRY table (16 bytes each)
    let mut offset = 6u32 + (entries.len() as u32) * 16;
    for (size, dib) in &entries {
        // Per ICO specification, 256px is encoded as 0 in ICONDIRENTRY width/height bytes.
        let dim = if *size >= 256 { 0u8 } else { *size as u8 };
        ico.push(dim);
        ico.push(dim);
        ico.push(0); // color count
        ico.push(0); // reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bit count
        ico.extend_from_slice(&(dib.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += dib.len() as u32;
    }

    for (_, dib) in entries {
        ico.extend_from_slice(&dib);
    }
    ico
}

fn emit_build_metadata() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    let sha = std::env::var("GITHUB_SHA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short=7", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
        })
        .map(|s| s.trim().chars().take(7).collect::<String>())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "local".to_string());
    println!("cargo:rustc-env=APP_BUILD_SHA={sha}");
}

#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    emit_build_metadata();
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    let ico_path = out_dir.join("app.ico");
    std::fs::write(&ico_path, build_multi_res_ico(&[16, 24, 32, 48, 64, 128, 256]))
        .expect("failed to write generated app.ico");

    // Forward slashes prevent Windows rc.exe from treating `\t` in `\target` as an escape sequence.
    let ico_str = ico_path.to_string_lossy().replace('\\', "/");
    let mut res = winres::WindowsResource::new();
    res.set_icon(&ico_str);
    res.set("ProductName", "Valorant Streamer Radar");
    res.set("FileDescription", "Valorant Streamer Radar");
    res.compile().expect("failed to compile Windows icon resource");
}

#[cfg(not(windows))]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    emit_build_metadata();
}
