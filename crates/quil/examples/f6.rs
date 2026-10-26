fn main() {
    let _ = rdev::simulate(&rdev::EventType::KeyPress(rdev::Key::F6));
    let _ = rdev::simulate(&rdev::EventType::KeyRelease(rdev::Key::F6));
}
