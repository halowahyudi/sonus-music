/// Volume in the range 0.0 to 1.0.
///
/// Wraps the `f32` so an out-of-range value can't reach rodio, and so the
/// perceptual mapping lives in one place instead of at every call site.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume(f32);

impl Volume {
    pub const MIN: Volume = Volume(0.0);
    pub const MAX: Volume = Volume(1.0);

    pub fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    /// The linear gain handed to rodio.
    pub fn as_f32(&self) -> f32 {
        self.0
    }

    /// Bump by a delta, clamping to [0, 1].
    pub fn adjust(&mut self, delta: f32) {
        self.0 = (self.0 + delta).clamp(0.0, 1.0);
    }

    /// Step up or down by one "notch" — what the volume keys and the on-screen
    /// +/- buttons use. 5% feels right: fine enough to dial in, coarse enough
    /// that holding the key moves audibly.
    pub fn step_up(&mut self) {
        self.adjust(Self::STEP);
    }

    pub fn step_down(&mut self) {
        self.adjust(-Self::STEP);
    }

    pub const STEP: f32 = 0.05;

    pub fn percent(&self) -> u8 {
        (self.0 * 100.0).round() as u8
    }

    /// Gain mapped through a cube curve.
    ///
    /// A linear slider feels wrong: the top half of the range sounds almost
    /// identical and the bottom half drops off a cliff. Cubing the value
    /// spreads the perceived change evenly across the slider's travel.
    pub fn perceptual_gain(&self) -> f32 {
        self.0 * self.0 * self.0
    }

    pub fn is_muted(&self) -> bool {
        self.0 <= f32::EPSILON
    }
}

impl Default for Volume {
    fn default() -> Self {
        // Loud enough to hear, quiet enough not to startle.
        Volume(0.7)
    }
}

impl From<f32> for Volume {
    fn from(v: f32) -> Self {
        Self::new(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_out_of_range() {
        assert_eq!(Volume::new(-1.0).as_f32(), 0.0);
        assert_eq!(Volume::new(2.0).as_f32(), 1.0);
        assert_eq!(Volume::new(0.5).as_f32(), 0.5);
    }

    #[test]
    fn default_is_seventy_percent() {
        assert_eq!(Volume::default().percent(), 70);
    }

    #[test]
    fn adjust_clamps_at_both_ends() {
        let mut v = Volume::new(0.98);
        v.adjust(0.5);
        assert_eq!(v.as_f32(), 1.0);

        v.adjust(-5.0);
        assert_eq!(v.as_f32(), 0.0);
    }

    #[test]
    fn stepping_moves_by_one_notch() {
        let mut v = Volume::new(0.5);
        v.step_up();
        assert_eq!(v.percent(), 55);
        v.step_down();
        assert_eq!(v.percent(), 50);
    }

    #[test]
    fn stepping_does_not_overshoot() {
        let mut v = Volume::MAX;
        v.step_up();
        assert_eq!(v.as_f32(), 1.0);

        let mut v = Volume::MIN;
        v.step_down();
        assert_eq!(v.as_f32(), 0.0);
    }

    #[test]
    fn percent_rounds() {
        assert_eq!(Volume::new(0.0).percent(), 0);
        assert_eq!(Volume::new(1.0).percent(), 100);
        assert_eq!(Volume::new(0.333).percent(), 33);
        assert_eq!(Volume::new(0.335).percent(), 34);
    }

    #[test]
    fn perceptual_gain_is_monotonic_and_bounded() {
        assert_eq!(Volume::new(0.0).perceptual_gain(), 0.0);
        assert_eq!(Volume::new(1.0).perceptual_gain(), 1.0);

        let mut previous = 0.0;
        for i in 0..=100 {
            let gain = Volume::new(i as f32 / 100.0).perceptual_gain();
            assert!(gain >= previous, "gain must not decrease");
            assert!((0.0..=1.0).contains(&gain));
            previous = gain;
        }
    }

    #[test]
    fn perceptual_gain_quiets_the_middle() {
        // 50% on the slider should be well under half the amplitude.
        assert!(Volume::new(0.5).perceptual_gain() < 0.2);
    }

    #[test]
    fn mute_detection() {
        assert!(Volume::new(0.0).is_muted());
        assert!(!Volume::new(0.01).is_muted());
    }

    #[test]
    fn converts_from_f32() {
        assert_eq!(Volume::from(0.4).as_f32(), 0.4);
        assert_eq!(Volume::from(9.0).as_f32(), 1.0);
    }
}
