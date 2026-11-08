//! Icone d'etat : l'icone de l'appli avec une pastille (rouge = micro coupe, orange = coupe d'une sortie, gris = hors ligne).
pub fn badge(base: &[u8], size: u32, colour: Option<(u8, u8, u8)>) -> Vec<u8> {
    let mut px = base.to_vec();
    let Some((r, g, b)) = colour else { return px };
    let s = size as f32;
    let (cx, cy, rad) = (s - 9.0, s - 9.0, 8.0);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let i = ((y * size + x) * 4) as usize;
            if d <= rad + 1.5 {
                let (pr, pg, pb) = if d <= rad { (r, g, b) } else { (20, 20, 24) };
                let a = if d <= rad { 1.0 } else { (rad + 1.5 - d).clamp(0.0, 1.0) };
                blend(&mut px[i..i + 4], pr, pg, pb, a);
                // trait blanc en diagonale (comme le bouton de toux)
                let on_line = ((fx - cx) - (fy - cy)).abs() < 1.3 && d < rad - 2.5;
                if on_line {
                    blend(&mut px[i..i + 4], 255, 255, 255, 1.0);
                }
            }
        }
    }
    px
}

fn blend(p: &mut [u8], r: u8, g: u8, b: u8, a: f32) {
    let mix = |o: u8, n: u8| (o as f32 * (1.0 - a) + n as f32 * a).round() as u8;
    p[0] = mix(p[0], r);
    p[1] = mix(p[1], g);
    p[2] = mix(p[2], b);
    p[3] = p[3].max((a * 255.0) as u8);
}
