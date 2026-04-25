use std::time::Duration;

use rand::Rng;

pub fn jitter_click_point(center: (i32, i32), radius: u32) -> (i32, i32) {
    if radius == 0 {
        return center;
    }
    // radius が i32::MAX を超えると `as i32` で負値化し、後続の gen_range(-r..=r) が
    // 逆順 range で panic するため、安全側に飽和させる。
    let r = radius.min(i32::MAX as u32) as i32;
    let mut rng = rand::thread_rng();
    let dx = rng.gen_range(-r..=r);
    let dy = rng.gen_range(-r..=r);
    (center.0 + dx, center.1 + dy)
}

pub fn random_press_duration_ms(min_ms: u64, max_ms: u64) -> u64 {
    if max_ms <= min_ms {
        return min_ms;
    }
    rand::thread_rng().gen_range(min_ms..=max_ms)
}

pub fn random_delay(min_ms: u64, max_ms: u64) -> Duration {
    Duration::from_millis(random_press_duration_ms(min_ms, max_ms))
}
