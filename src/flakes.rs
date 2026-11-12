//! Particle model: one `FlakeField` per monitor, holding parallel arrays that
//! are advanced once per frame. Coordinates are monitor pixels; `(base_x, y)`
//! is the top-left corner of a flake's bounding square.

use std::f32::consts::TAU;

/// Angular speed of the horizontal sway, in radians per second.
pub const SWAY_FREQ: f64 = 0.8;

/// xorshift64* generator. Statistical quality does not matter for snow, and
/// this keeps the hot loop free of external calls.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    /// Uniform value in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / 16_777_216.0
    }

    /// Uniform value in `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

pub struct FlakeField {
    width: f32,
    height: f32,
    base_x: Vec<f32>,
    y: Vec<f32>,
    vy: Vec<f32>,
    phase: Vec<f32>,
}

impl FlakeField {
    /// Half of the flakes start above the screen and drift in, as before.
    pub fn new(width: f32, height: f32, count: usize, speed: f32, rng: &mut Rng) -> FlakeField {
        let mut field = FlakeField {
            width,
            height,
            base_x: Vec::with_capacity(count),
            y: Vec::with_capacity(count),
            vy: Vec::with_capacity(count),
            phase: Vec::with_capacity(count),
        };
        for _ in 0..count {
            let y = rng.range(-height, height);
            field.push(y, speed, rng);
        }
        field
    }

    fn push(&mut self, y: f32, speed: f32, rng: &mut Rng) {
        self.base_x.push(rng.range(0.0, self.width));
        self.y.push(y);
        self.vy.push(speed * rng.range(0.7, 1.3));
        self.phase.push(rng.range(0.0, TAU));
    }

    pub fn count(&self) -> usize {
        self.y.len()
    }

    /// Shrinks by dropping the newest flakes; grows with flakes that start
    /// above the screen so they fall into view.
    pub fn set_count(&mut self, count: usize, speed: f32, rng: &mut Rng) {
        if count <= self.count() {
            self.base_x.truncate(count);
            self.y.truncate(count);
            self.vy.truncate(count);
            self.phase.truncate(count);
        } else {
            for _ in self.count()..count {
                let y = rng.range(-self.height, 0.0);
                self.push(y, speed, rng);
            }
        }
    }

    pub fn set_speed(&mut self, speed: f32, rng: &mut Rng) {
        for vy in &mut self.vy {
            *vy = speed * rng.range(0.7, 1.3);
        }
    }

    /// Advances every flake by `dt` seconds and writes the disc centres as
    /// `x, y` pairs, multiplied by `scale`, into `out`.
    ///
    /// `sway_phase` is the common sway angle (`time * SWAY_FREQ`, wrapped to one
    /// turn by the caller so precision does not degrade over long uptimes).
    /// Allocation-free: this runs once per frame.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        dt: f32,
        sway_phase: f32,
        size: f32,
        sway_amp: f32,
        scale: f32,
        rng: &mut Rng,
        out: &mut [f32],
    ) {
        let (width, height) = (self.width, self.height);
        let half = size * 0.5;
        let fall = dt * 60.0;
        let flakes = self
            .base_x
            .iter_mut()
            .zip(self.y.iter_mut())
            .zip(self.vy.iter())
            .zip(self.phase.iter_mut());
        for ((((base_x, y), vy), phase), centre) in flakes.zip(out.chunks_exact_mut(2)) {
            let mut ny = *y + *vy * fall;
            if ny > height {
                ny = -size;
                *base_x = rng.range(0.0, width);
                *phase = rng.range(0.0, TAU);
            }
            *y = ny;
            let nx = *base_x + (*phase + sway_phase).sin() * sway_amp;
            centre[0] = (nx + half) * scale;
            centre[1] = (ny + half) * scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 1920.0;
    const H: f32 = 1080.0;

    #[test]
    fn new_field_has_the_requested_count_within_bounds() {
        let mut rng = Rng::new(7);
        let field = FlakeField::new(W, H, 500, 0.3, &mut rng);
        assert_eq!(field.count(), 500);
        assert!(field.base_x.iter().all(|x| (0.0..W).contains(x)));
        assert!(field.y.iter().all(|y| (-H..H).contains(y)));
        assert!(field.vy.iter().all(|v| (0.3 * 0.7..0.3 * 1.3).contains(v)));
        assert!(field.phase.iter().all(|p| (0.0..TAU).contains(p)));
    }

    #[test]
    fn step_falls_by_speed_and_reports_the_scaled_centre() {
        let mut rng = Rng::new(1);
        let mut field = FlakeField::new(W, H, 1, 1.0, &mut rng);
        field.base_x[0] = 100.0;
        field.y[0] = 200.0;
        field.vy[0] = 2.0;
        field.phase[0] = 0.0;
        let mut out = [0.0f32; 2];
        // 0.5 s at 2 px per 1/60 s = 60 px; sway angle pi/2 gives the full amplitude.
        field.step(
            0.5,
            std::f32::consts::FRAC_PI_2,
            18.0,
            10.0,
            0.5,
            &mut rng,
            &mut out,
        );
        assert!((field.y[0] - 260.0).abs() < 1e-3);
        assert!((out[0] - (100.0 + 10.0 + 9.0) * 0.5).abs() < 1e-3);
        assert!((out[1] - (260.0 + 9.0) * 0.5).abs() < 1e-3);
    }

    #[test]
    fn flake_leaving_the_bottom_restarts_above_the_top() {
        let mut rng = Rng::new(3);
        let mut field = FlakeField::new(W, H, 1, 1.0, &mut rng);
        field.y[0] = H - 1.0;
        field.vy[0] = 1.0;
        let mut out = [0.0f32; 2];
        field.step(1.0, 0.0, 18.0, 0.0, 1.0, &mut rng, &mut out);
        assert_eq!(field.y[0], -18.0);
        assert!((0.0..W).contains(&field.base_x[0]));
        assert!((0.0..TAU).contains(&field.phase[0]));
    }

    #[test]
    fn set_count_grows_above_the_screen_and_shrinks_in_place() {
        let mut rng = Rng::new(5);
        let mut field = FlakeField::new(W, H, 10, 0.3, &mut rng);
        field.set_count(25, 0.3, &mut rng);
        assert_eq!(field.count(), 25);
        assert!(field.y[10..].iter().all(|y| (-H..0.0).contains(y)));
        field.set_count(4, 0.3, &mut rng);
        for len in [
            field.base_x.len(),
            field.y.len(),
            field.vy.len(),
            field.phase.len(),
        ] {
            assert_eq!(len, 4);
        }
    }

    #[test]
    fn set_speed_rerolls_every_velocity() {
        let mut rng = Rng::new(9);
        let mut field = FlakeField::new(W, H, 50, 0.3, &mut rng);
        field.set_speed(5.0, &mut rng);
        assert!(field.vy.iter().all(|v| (5.0 * 0.7..5.0 * 1.3).contains(v)));
    }

    #[test]
    fn step_only_writes_as_many_centres_as_there_are_flakes() {
        let mut rng = Rng::new(11);
        let mut field = FlakeField::new(W, H, 3, 0.3, &mut rng);
        let mut out = [-1.0f32; 10];
        field.step(0.01, 0.0, 6.0, 0.0, 1.0, &mut rng, &mut out);
        assert!(out[6..].iter().all(|v| *v == -1.0));
    }
}
