/// Windows reports per-session volume as a scalar from 0.0 to 1.0.
///
/// In default-only mode, we only touch sessions that are effectively at
/// Windows' initial 100% value. In cap mode, every newly observed session
/// above the configured target is lowered. This function never raises volume.
pub(crate) fn desired_volume(current: f32, target: f32, cap: bool) -> Option<f32> {
    if !current.is_finite() || !target.is_finite() {
        return None;
    }

    let current = current.clamp(0.0, 1.0);
    let target = target.clamp(0.0, 1.0);

    if current <= target {
        return None;
    }

    const WINDOWS_DEFAULT_THRESHOLD: f32 = 0.995;
    if cap || current >= WINDOWS_DEFAULT_THRESHOLD {
        Some(target)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::desired_volume;

    #[test]
    fn lowers_a_default_session() {
        assert_eq!(desired_volume(1.0, 0.3, false), Some(0.3));
    }

    #[test]
    fn preserves_a_remembered_manual_setting() {
        assert_eq!(desired_volume(0.5, 0.3, false), None);
    }

    #[test]
    fn cap_mode_lowers_any_session_above_target() {
        assert_eq!(desired_volume(0.5, 0.3, true), Some(0.3));
    }

    #[test]
    fn never_raises_a_quiet_session() {
        assert_eq!(desired_volume(0.2, 0.3, true), None);
    }

    #[test]
    fn rejects_non_finite_input() {
        assert_eq!(desired_volume(f32::NAN, 0.3, false), None);
    }
}
