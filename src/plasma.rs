use embassy_executor::task;
use embassy_stm32::bind_interrupts;
use embassy_stm32::exti::{ExtiInput, InterruptHandler};
use embassy_stm32::gpio::Pull;
use embassy_stm32::interrupt::typelevel::EXTI15_10 as IrqExti15_10;
use embassy_stm32::peripherals::{EXTI12, PD12};
use embassy_stm32::Peri;

use crate::hardware::mdma::MdmaCh0;
use crate::hardware::st7796::{St7796, HEIGHT, WIDTH};

bind_interrupts!(pub struct TeIrqs {
    EXTI15_10 => InterruptHandler<IrqExti15_10>;
});

const W: usize = WIDTH as usize;
const HALF: usize = W / 2;
const HORIZON: u16 = HEIGHT / 2;

// DMA_BUFFERS lines are 480 pixels wide
const _: () = assert!(W <= 480);

/// A double-buffer containing two 480-pixel 16-bit lines (1920 bytes total).
/// Must be exactly padded and aligned to 2048 bytes for the Cortex-M7 MPU region
/// so that surrounding variables do not accidentally become un-cached.
#[repr(C, align(2048))]
pub struct LineBuffers {
    pub buf_a: [u16; 480],
    pub buf_b: [u16; 480],
    pub _pad: [u16; 1024 - 960], // Pad remainder up to 2048 bytes
}

pub static mut DMA_BUFFERS: LineBuffers = LineBuffers {
    buf_a: [0; 480],
    buf_b: [0; 480],
    _pad: [0; 64],
};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

const fn rgb(r: u8, g: u8, b: u8) -> u16 {
    (((r as u16) >> 3) << 11) | (((g as u16) >> 2) << 5) | ((b as u16) >> 3)
}

const SKY0: u16 = rgb(2, 0, 18); // zenith
const SKY1: u16 = rgb(34, 4, 86);
const SKY2: u16 = rgb(132, 18, 128);
const SKY3: u16 = rgb(255, 104, 92); // horizon glow

const HALO: u16 = rgb(255, 70, 170);
const SUN_TOP: u16 = rgb(255, 236, 100);
const SUN_MID: u16 = rgb(255, 128, 56);
const SUN_BOT: u16 = rgb(255, 28, 140);

const STAR_WHITE: u16 = rgb(255, 255, 255);
const STAR_CYAN: u16 = rgb(170, 235, 255);
const STAR_PINK: u16 = rgb(255, 180, 235);

const FAR_TOP: u16 = rgb(28, 6, 64);
const FAR_BOT: u16 = rgb(112, 24, 124);
const FAR_EDGE: u16 = rgb(200, 110, 255);
const NEAR_TOP: u16 = rgb(10, 2, 26);
const NEAR_BOT: u16 = rgb(46, 8, 72);
const NEAR_EDGE: u16 = rgb(255, 48, 208);

const GROUND_HORIZON: u16 = rgb(120, 18, 120);
const GROUND_DARK: u16 = rgb(8, 2, 26);
const GRID_COLOR: u16 = rgb(255, 50, 230);
const HORIZON_LINE: u16 = rgb(255, 170, 110);
const SUN_REFLECT: u16 = rgb(255, 110, 150);

// ---------------------------------------------------------------------------
// Scene constants
// ---------------------------------------------------------------------------

const SUN_CX: i32 = (W / 2) as i32;
const SUN_CY: i32 = 118;
const SUN_R: i32 = 64;
const SUN_RH: i32 = SUN_R + 44; // halo radius
const SUN_R2: i32 = SUN_R * SUN_R;
const SUN_RH2: i32 = SUN_RH * SUN_RH;

const GRID_K: u32 = 2_048_000; // perspective scale
const GRID_G: u32 = 15_360; // world grid spacing; scroll offset wraps at exactly this

const NSTARS: usize = 110;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Blend two RGB565 colors. t = 0..=32 (0 = a, 32 = b).
#[inline(always)]
fn mix(a: u16, b: u16, t: u32) -> u16 {
    const MASK: u32 = 0x07E0_F81F;
    let a = a as u32;
    let b = b as u32;
    let a = (a | (a << 16)) & MASK;
    let b = (b | (b << 16)) & MASK;
    let r = ((a * (32 - t) + b * t) >> 5) & MASK;
    (r | (r >> 16)) as u16
}

fn isqrt(n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    let mut r = 0;
    let mut b = 128;
    while b > 0 {
        let t = r + b;
        if t * t <= n {
            r = t;
        }
        b >>= 1;
    }
    r
}

fn next_rand(s: &mut u32) -> u32 {
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *s = x;
    x
}

/// Generates a fast 256-entry sine wave LUT (values 0..=64) using integer math
fn generate_sin_lut() -> [u8; 256] {
    let mut lut = [0u8; 256];
    for i in 0..256 {
        let phase = i & 127;
        let val = if phase < 64 { phase } else { 128 - phase };
        let curve = (val * val) / 64;
        lut[i] = if i < 128 {
            (32 + curve / 2) as u8
        } else {
            (32 - curve / 2) as u8
        };
    }
    lut
}

fn sky_color(y: u16) -> u16 {
    let t = (y as u32 * 96) / HORIZON as u32; // 0..95
    if t < 32 {
        mix(SKY0, SKY1, t)
    } else if t < 64 {
        mix(SKY1, SKY2, t - 32)
    } else {
        mix(SKY2, SKY3, t - 64)
    }
}

// ---------------------------------------------------------------------------
// Scene state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Star {
    x: u16,
    y: u16,
    phase: u8,
    kind: u8, // 0-4 white, 5 cyan, 6 pink, 7 big white (cross)
}

struct Scene {
    sin_lut: [u8; 256],
    stars: [Star; NSTARS],
    far_top: [u16; W],
    near_top: [u16; W],
    peak: u16, // highest mountain pixel row this frame
}

impl Scene {
    fn new() -> Self {
        let mut rng: u32 = 0x1234_5678;
        let mut stars = [Star { x: 0, y: 0, phase: 0, kind: 0 }; NSTARS];
        for s in stars.iter_mut() {
            s.x = 2 + (next_rand(&mut rng) % (W as u32 - 4)) as u16;
            s.y = (next_rand(&mut rng) % (HORIZON as u32 - 30)) as u16;
            s.phase = next_rand(&mut rng) as u8;
            s.kind = ((next_rand(&mut rng) >> 8) % 8) as u8;
        }
        Self {
            sin_lut: generate_sin_lut(),
            stars,
            far_top: [HORIZON; W],
            near_top: [HORIZON; W],
            peak: HORIZON,
        }
    }

    /// Recompute the two parallax mountain ridgelines for this frame.
    fn update(&mut self, frame: u32) {
        let mut peak = HORIZON;
        for x in 0..W {
            let xi = x as u32;
            let a = self.sin_lut[((xi / 3).wrapping_add(frame >> 4) & 255) as usize] as u32;
            let b = self.sin_lut[((xi * 5 / 4 + 90).wrapping_add(frame / 10) & 255) as usize] as u32;
            let c = self.sin_lut[((xi * 2).wrapping_add(frame) & 255) as usize] as u32;
            let d = self.sin_lut[((xi * 3 + 37).wrapping_add(frame) & 255) as usize] as u32;

            let far_h = 14 + a / 2 + b / 4; // 14..62
            let near_h = 6 + c / 4 + d / 4; // 6..38

            let ft = HORIZON - far_h as u16;
            let nt = HORIZON - near_h as u16;
            self.far_top[x] = ft;
            self.near_top[x] = nt;
            peak = peak.min(ft).min(nt);
        }
        self.peak = peak;
    }
}

// ---------------------------------------------------------------------------
// Line rendering
// ---------------------------------------------------------------------------

fn halo_span(buf: &mut [u16], a: usize, b: usize, dy2: i32, pulse: u32) {
    for x in a..b {
        let dx = x as i32 - SUN_CX;
        let d2 = dx * dx + dy2;
        if d2 >= SUN_RH2 {
            continue;
        }
        let s = (SUN_RH2 - d2) as u32;
        let t = s * 32 / SUN_RH2 as u32; // 0..32, linear in d^2
        let t = t * t / 32 * pulse / 32; // squared falloff, scaled by pulse
        buf[x] = mix(buf[x], HALO, t);
    }
}

fn sky_line(buf: &mut [u16], y: u16, frame: u32, sc: &Scene) {
    let buf = &mut buf[..W];
    let sky = sky_color(y);
    buf.fill(sky);

    // --- Twinkling stars (fade out toward the horizon glow) ---
    let fade = ((HORIZON - y) as u32 * 32 / HORIZON as u32).min(32);
    for s in sc.stars.iter() {
        let dy = y as i32 - s.y as i32;
        let big = s.kind == 7;
        if dy.abs() > 1 || (dy != 0 && !big) {
            continue;
        }
        let speed = (s.phase as u32 & 3) + 1;
        let idx = (s.phase as u32).wrapping_add(frame.wrapping_mul(speed)) & 255;
        let tw = sc.sin_lut[idx as usize] as u32 / 2; // 0..=32
        let b = (tw * tw / 32) * fade / 32;
        let col = match s.kind {
            5 => STAR_CYAN,
            6 => STAR_PINK,
            _ => STAR_WHITE,
        };
        let x = s.x as usize;
        if dy == 0 {
            buf[x] = mix(buf[x], col, b);
            if big {
                buf[x - 1] = mix(buf[x - 1], col, b / 2);
                buf[x + 1] = mix(buf[x + 1], col, b / 2);
            }
        } else {
            buf[x] = mix(buf[x], col, b / 2);
        }
    }

    // --- Shooting star: one every 360 frames, lasting 40 ---
    let cycle = frame / 360;
    let t = (frame % 360) as i32;
    if t < 40 {
        let seed = cycle.wrapping_mul(2_654_435_761).wrapping_add(0x9E37_79B9);
        let sx = 300 + ((seed >> 7) % 160) as i32;
        let sy = 8 + ((seed >> 19) % 50) as i32;
        let hx = sx - t * 8;
        let hy = sy + t * 2;
        let life = if t < 8 {
            t * 4
        } else if t > 30 {
            (40 - t) * 3
        } else {
            32
        } as u32;
        for i in 0..40i32 {
            if hy - i / 2 != y as i32 {
                continue;
            }
            let px = hx + i * 2;
            let b = ((32 - i * 32 / 40) as u32) * life / 32;
            for k in 0..3 {
                let p = px + k;
                if p >= 0 && (p as usize) < W {
                    let p = p as usize;
                    buf[p] = mix(buf[p], STAR_WHITE, b);
                }
            }
        }
    }

    // --- Sun halo + striped sun ---
    let dy = y as i32 - SUN_CY;
    let dy2 = dy * dy;
    if dy2 < SUN_RH2 {
        let hwh = isqrt(SUN_RH2 - dy2);
        let hw = if dy2 < SUN_R2 { isqrt(SUN_R2 - dy2) } else { 0 };
        let pulse = 18 + sc.sin_lut[((frame >> 1) & 255) as usize] as u32 / 8; // 18..26

        let x0 = (SUN_CX - hwh).max(0) as usize;
        let x1 = ((SUN_CX + hwh) as usize).min(W);
        let sl = (SUN_CX - hw).max(0) as usize;
        let sr = ((SUN_CX + hw) as usize).min(W);

        halo_span(buf, x0, sl, dy2, pulse);
        halo_span(buf, sr, x1, dy2, pulse);

        if hw > 0 {
            // Vertical gradient: yellow -> orange -> hot pink
            let ys = (y as i32 - (SUN_CY - SUN_R)).max(0) as u32;
            let g = (ys * 64 / (2 * SUN_R as u32)).min(63);
            let base = if g < 32 {
                mix(SUN_TOP, SUN_MID, g)
            } else {
                mix(SUN_MID, SUN_BOT, g - 32)
            };

            // Scrolling cut-out stripes, thicker toward the bottom
            let cut_y = SUN_CY - 12;
            let is_cut = if (y as i32) > cut_y {
                let th = (((y as i32 - cut_y) * 3 / 10).min(22)) as u32;
                ((y as u32).wrapping_sub(frame >> 1) & 31) < th
            } else {
                false
            };

            if !is_cut {
                buf[sl..sr].fill(base);
            }
        }
    }

    // --- Parallax mountains (far hazy layer + near dark layer with neon edge) ---
    if y >= sc.peak {
        let hz = (y as u32 * 32 / HORIZON as u32).min(32);
        let far_fill = mix(FAR_TOP, FAR_BOT, hz);
        let near_fill = mix(NEAR_TOP, NEAR_BOT, hz);
        for x in 0..W {
            let nt = sc.near_top[x];
            if y >= nt {
                buf[x] = if y < nt + 2 { NEAR_EDGE } else { near_fill };
            } else {
                let ft = sc.far_top[x];
                if y >= ft {
                    buf[x] = if y < ft + 1 { FAR_EDGE } else { far_fill };
                }
            }
        }
    }
}

fn ground_line(buf: &mut [u16], y: u16, frame: u32) {
    let buf = &mut buf[..W];
    let dy = (y - HORIZON + 1) as u32; // 1..=HORIZON

    // Bright thin line right at the horizon
    if dy == 1 {
        buf.fill(HORIZON_LINE);
        return;
    }

    // Ground haze: pink-violet at the horizon fading to dark blue
    let bg = mix(GROUND_HORIZON, GROUND_DARK, (dy / 2).min(32));
    let fade = (dy * 32 / 72).min(32);
    let line = mix(bg, GRID_COLOR, fade);

    if dy < 10 {
        // Too dense to resolve this close to the horizon; avoids moire
        buf.fill(bg);
    } else {
        // Horizontal lines scrolling toward the viewer, thicker when closer
        let th = 1 + dy / 40;
        let time_offset = (frame % 128) * 120; // wraps at exactly GRID_G
        let z_curr = GRID_K / dy + time_offset;
        let z_next = GRID_K / (dy + th) + time_offset;

        if z_curr / GRID_G != z_next / GRID_G {
            buf.fill(line);
        } else {
            // Vertical lines fanning out from the vanishing point
            let d = dy as i32;
            let vw = (6 + dy / 16) as i32; // line width grows with proximity

            let mut v = (HALF as i32 * 6) % d;
            for x in 0..HALF {
                buf[x] = if v < vw { line } else { bg };
                v -= 6;
                while v < 0 {
                    v += d;
                }
            }

            let mut v = 0i32;
            for x in HALF..W {
                buf[x] = if v < vw { line } else { bg };
                v += 6;
                while v >= d {
                    v -= d;
                }
            }
        }
    }

    // Shimmering sun reflection on the ground, broken into flickering scanlines
    if dy <= 56 && ((dy.wrapping_add(frame >> 2)) & 3) != 3 {
        let k = 56 - dy;
        let hw = (k * 5 / 4) as i32;
        let s = (k * 20 / 56) as i32;
        let x0 = (SUN_CX - hw).max(0);
        let x1 = (SUN_CX + hw).min(W as i32);
        for x in x0..x1 {
            let a = hw - (x - SUN_CX).abs();
            let t = (s * a / hw) as u32;
            let xu = x as usize;
            buf[xu] = mix(buf[xu], SUN_REFLECT, t);
        }
    }
}

#[inline(always)]
fn compute_line(buf: &mut [u16], y: u16, frame: u32, sc: &Scene) {
    if y < HORIZON {
        sky_line(buf, y, frame, sc);
    } else {
        ground_line(buf, y, frame);
    }
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

#[task]
pub async fn plasma_task(
    mut display: St7796,
    te_pin: Peri<'static, PD12>,
    exti: Peri<'static, EXTI12>,
    mut mdma: MdmaCh0,
) {
    use defmt::error;
    use embassy_time::{with_timeout, Duration};

    let mut te = ExtiInput::new(te_pin, exti, Pull::None, TeIrqs);

    let mut scene = Scene::new();
    let mut frame: u32 = 0;

    // Clear GRAM to black while the panel is still off, so no power-on noise is shown
    {
        let bufs = unsafe { core::ptr::addr_of_mut!(DMA_BUFFERS).as_mut().unwrap() };
        bufs.buf_a.fill(0);
        display.set_window(0, 0, WIDTH - 1, HEIGHT - 1);
        for y in 0..HEIGHT {
            mdma.start_memory_to_peripheral_16bit(
                bufs.buf_a.as_ptr(),
                St7796::data_ptr(),
                W as u32,
            );
            if with_timeout(Duration::from_millis(10), mdma.wait_complete())
                .await
                .is_err()
            {
                error!("MDMA Timeout on clear line {}!", y);
                break;
            }
        }
    }
    display.display_on().await;

    loop {
        scene.update(frame);
        display.set_window(0, 0, WIDTH - 1, HEIGHT - 1);

        let bufs = unsafe { core::ptr::addr_of_mut!(DMA_BUFFERS).as_mut().unwrap() };
        let dest_ptr = St7796::data_ptr();
        let ptrs = [bufs.buf_a.as_mut_ptr(), bufs.buf_b.as_mut_ptr()];
        let line = |i: usize| unsafe { core::slice::from_raw_parts_mut(ptrs[i & 1], W) };

        // Prepare line 0 first so streaming starts the moment TE fires
        compute_line(line(0), 0, frame, &scene);

        // TE mode 1 pulses at V-blank; start writing on the rising edge
        te.wait_for_rising_edge().await;

        for y in 0..HEIGHT {
            mdma.start_memory_to_peripheral_16bit(ptrs[y as usize & 1], dest_ptr, W as u32);

            // Render the next line into the other buffer while the DMA runs
            if y + 1 < HEIGHT {
                compute_line(line(y as usize + 1), y + 1, frame, &scene);
            }

            match with_timeout(Duration::from_millis(10), mdma.wait_complete()).await {
                Ok(_) => {}
                Err(_) => {
                    error!("MDMA Timeout on line {}!", y);
                    break;
                }
            }
        }

        frame = frame.wrapping_add(1);
    }
}
