fn main() {
    if video_provider::run_camera_worker().is_err() {
        std::process::exit(1);
    }
}
