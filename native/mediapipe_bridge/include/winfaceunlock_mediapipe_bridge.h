#pragma once

#include <cstddef>
#include <cstdint>

#if defined(_WIN32)
#if defined(WINFACEUNLOCK_MEDIAPIPE_BRIDGE_EXPORTS)
#define WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API __declspec(dllexport)
#else
#define WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API __declspec(dllimport)
#endif
#else
#define WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API __attribute__((visibility("default")))
#endif

extern "C" {

struct WinFaceUnlockMediaPipeOptions {
  std::uint32_t running_mode;
  std::uint8_t output_face_blendshapes;
  std::uint8_t output_facial_transformation_matrixes;
  std::uint8_t reserved[6];
};

struct WinFaceUnlockMediaPipeFrameRequest {
  std::uint32_t width;
  std::uint32_t height;
  std::uint32_t pixel_format;
  std::uint32_t reserved;
  const std::uint8_t* data;
  std::size_t data_len;
  float face_box_x;
  float face_box_y;
  float face_box_width;
  float face_box_height;
};

struct WinFaceUnlockMediaPipePoseResult {
  float yaw_deg;
  float pitch_deg;
  float roll_deg;
  float left_eye_blink_score;
  float right_eye_blink_score;
};

struct WinFaceUnlockMediaPipePresencePoseOptions {
  std::uint32_t running_mode;
  float min_landmark_visibility;
  float min_landmark_presence;
};

struct WinFaceUnlockMediaPipePresencePoseResult {
  std::uint8_t detected;
  std::uint8_t reserved[3];
  float confidence;
  float bbox_center_x_ratio;
  float bbox_area_ratio;
  float normalized_x_min;
  float normalized_y_min;
  float normalized_x_max;
  float normalized_y_max;
};

WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API void*
winfaceunlock_mediapipe_pose_create(
    const char* model_path,
    WinFaceUnlockMediaPipeOptions options);

WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API void
winfaceunlock_mediapipe_pose_destroy(void* provider);

WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API int
winfaceunlock_mediapipe_pose_estimate(
    void* provider,
    const WinFaceUnlockMediaPipeFrameRequest* request,
    WinFaceUnlockMediaPipePoseResult* result);

WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API void*
winfaceunlock_mediapipe_presence_pose_create(
    const char* model_path,
    WinFaceUnlockMediaPipePresencePoseOptions options);

WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API void
winfaceunlock_mediapipe_presence_pose_destroy(void* provider);

WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API int
winfaceunlock_mediapipe_presence_pose_estimate(
    void* provider,
    const WinFaceUnlockMediaPipeFrameRequest* request,
    WinFaceUnlockMediaPipePresencePoseResult* result);

}  // extern "C"
