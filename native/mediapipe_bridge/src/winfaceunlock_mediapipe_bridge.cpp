#include "winfaceunlock_mediapipe_bridge.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <memory>
#include <vector>

#include "mediapipe/tasks/c/core/base_options.h"
#include "mediapipe/tasks/c/core/common.h"
#include "mediapipe/tasks/c/core/mp_status.h"
#include "mediapipe/tasks/c/vision/core/image.h"
#include "mediapipe/tasks/c/vision/face_landmarker/face_landmarker.h"
#include "mediapipe/tasks/c/vision/pose_landmarker/pose_landmarker.h"

namespace {

constexpr std::uint32_t kPixelFormatBgr8 = 0;
constexpr std::uint32_t kPixelFormatRgb8 = 1;
constexpr std::uint32_t kPixelFormatGray8 = 2;

constexpr std::uint32_t kRunningModeImage = 0;
constexpr std::uint32_t kRunningModeVideo = 1;

constexpr float kRadiansToDegrees = 57.29577951308232F;

struct ProviderState {
  MpFaceLandmarkerPtr landmarker = nullptr;
  std::int64_t next_video_timestamp_ms = 0;
};

struct PresencePoseProviderState {
  MpPoseLandmarkerPtr landmarker = nullptr;
  std::int64_t next_video_timestamp_ms = 0;
  float min_landmark_visibility = 0.45F;
  float min_landmark_presence = 0.45F;
};

void FreeError(char* error_message) {
  if (error_message != nullptr) {
    MpErrorFree(error_message);
  }
}

MpRunningMode ToMediaPipeRunningMode(std::uint32_t running_mode) {
  if (running_mode == kRunningModeVideo) {
    return MP_RUNNING_MODE_VIDEO;
  }
  return MP_RUNNING_MODE_IMAGE;
}

bool RequestHasExpectedByteCount(const WinFaceUnlockMediaPipeFrameRequest& request) {
  const std::size_t pixel_count =
      static_cast<std::size_t>(request.width) * static_cast<std::size_t>(request.height);
  switch (request.pixel_format) {
    case kPixelFormatBgr8:
    case kPixelFormatRgb8:
      return request.data_len == pixel_count * 3;
    case kPixelFormatGray8:
      return request.data_len == pixel_count;
    default:
      return false;
  }
}

std::vector<std::uint8_t> ConvertBgrToRgb(const WinFaceUnlockMediaPipeFrameRequest& request) {
  std::vector<std::uint8_t> rgb(request.data_len);
  for (std::size_t index = 0; index < request.data_len; index += 3) {
    rgb[index] = request.data[index + 2];
    rgb[index + 1] = request.data[index + 1];
    rgb[index + 2] = request.data[index];
  }
  return rgb;
}

MpImagePtr CreateImage(const WinFaceUnlockMediaPipeFrameRequest& request) {
  if (request.data == nullptr || request.width == 0 || request.height == 0 ||
      !RequestHasExpectedByteCount(request)) {
    return nullptr;
  }

  MpImagePtr image = nullptr;
  char* error_message = nullptr;
  MpStatus status = kMpInvalidArgument;

  if (request.pixel_format == kPixelFormatBgr8) {
    std::vector<std::uint8_t> rgb = ConvertBgrToRgb(request);
    status = MpImageCreateFromUint8Data(
        kMpImageFormatSrgb,
        static_cast<int>(request.width),
        static_cast<int>(request.height),
        rgb.data(),
        static_cast<int>(rgb.size()),
        &image,
        &error_message);
  } else if (request.pixel_format == kPixelFormatRgb8) {
    status = MpImageCreateFromUint8Data(
        kMpImageFormatSrgb,
        static_cast<int>(request.width),
        static_cast<int>(request.height),
        request.data,
        static_cast<int>(request.data_len),
        &image,
        &error_message);
  } else if (request.pixel_format == kPixelFormatGray8) {
    status = MpImageCreateFromUint8Data(
        kMpImageFormatGray8,
        static_cast<int>(request.width),
        static_cast<int>(request.height),
        request.data,
        static_cast<int>(request.data_len),
        &image,
        &error_message);
  }

  FreeError(error_message);
  if (status != kMpOk) {
    return nullptr;
  }
  return image;
}

float CategoryScore(const MpCategories& categories, const char* category_name) {
  if (categories.categories == nullptr || category_name == nullptr) {
    return 0.0F;
  }
  for (std::uint32_t index = 0; index < categories.categories_count; ++index) {
    const MpCategory& category = categories.categories[index];
    if (category.category_name != nullptr &&
        std::strcmp(category.category_name, category_name) == 0) {
      return std::clamp(category.score, 0.0F, 1.0F);
    }
  }
  return 0.0F;
}

void ExtractBlinkScores(
    const MpFaceLandmarkerResult& landmark_result,
    WinFaceUnlockMediaPipePoseResult& result) {
  result.left_eye_blink_score = 0.0F;
  result.right_eye_blink_score = 0.0F;

  if (landmark_result.face_blendshapes == nullptr ||
      landmark_result.face_blendshapes_count == 0) {
    return;
  }

  const MpCategories& first_face_blendshapes = landmark_result.face_blendshapes[0];
  result.left_eye_blink_score = CategoryScore(first_face_blendshapes, "eyeBlinkLeft");
  result.right_eye_blink_score = CategoryScore(first_face_blendshapes, "eyeBlinkRight");
}

float MatrixAt(const MpMatrix& matrix, std::uint32_t row, std::uint32_t column) {
  return matrix.data[column * matrix.rows + row];
}

void ExtractPoseFromMatrix(
    const MpFaceLandmarkerResult& landmark_result,
    WinFaceUnlockMediaPipePoseResult& result) {
  result.yaw_deg = 0.0F;
  result.pitch_deg = 0.0F;
  result.roll_deg = 0.0F;

  if (landmark_result.facial_transformation_matrixes == nullptr ||
      landmark_result.facial_transformation_matrixes_count == 0) {
    return;
  }

  const MpMatrix& matrix = landmark_result.facial_transformation_matrixes[0];
  if (matrix.data == nullptr || matrix.rows < 3 || matrix.cols < 3) {
    return;
  }

  const float r00 = MatrixAt(matrix, 0, 0);
  const float r10 = MatrixAt(matrix, 1, 0);
  const float r20 = MatrixAt(matrix, 2, 0);
  const float r21 = MatrixAt(matrix, 2, 1);
  const float r22 = MatrixAt(matrix, 2, 2);

  result.yaw_deg = std::atan2(-r20, std::sqrt(r00 * r00 + r10 * r10)) * kRadiansToDegrees;
  result.pitch_deg = std::atan2(r21, r22) * kRadiansToDegrees;
  result.roll_deg = std::atan2(r10, r00) * kRadiansToDegrees;
}

bool LandmarkPassesGate(
    const MpNormalizedLandmark& landmark,
    const PresencePoseProviderState& state) {
  const bool visibility_passed =
      !landmark.has_visibility || landmark.visibility >= state.min_landmark_visibility;
  const bool presence_passed =
      !landmark.has_presence || landmark.presence >= state.min_landmark_presence;
  return visibility_passed && presence_passed && std::isfinite(landmark.x) &&
         std::isfinite(landmark.y) && landmark.x >= -0.2F && landmark.x <= 1.2F &&
         landmark.y >= -0.2F && landmark.y <= 1.2F;
}

float LandmarkConfidence(const MpNormalizedLandmark& landmark) {
  float confidence = 1.0F;
  if (landmark.has_visibility) {
    confidence = std::min(confidence, std::clamp(landmark.visibility, 0.0F, 1.0F));
  }
  if (landmark.has_presence) {
    confidence = std::min(confidence, std::clamp(landmark.presence, 0.0F, 1.0F));
  }
  return confidence;
}

void ExtractPresencePoseResult(
    const MpPoseLandmarkerResult& landmark_result,
    const PresencePoseProviderState& state,
    WinFaceUnlockMediaPipePresencePoseResult& result) {
  result = WinFaceUnlockMediaPipePresencePoseResult{};
  if (landmark_result.pose_landmarks == nullptr ||
      landmark_result.pose_landmarks_count == 0) {
    return;
  }

  const MpNormalizedLandmarks& first_pose = landmark_result.pose_landmarks[0];
  if (first_pose.landmarks == nullptr || first_pose.landmarks_count == 0) {
    return;
  }

  float x_min = 1.0F;
  float y_min = 1.0F;
  float x_max = 0.0F;
  float y_max = 0.0F;
  float confidence_sum = 0.0F;
  std::uint32_t accepted_count = 0;

  for (std::uint32_t index = 0; index < first_pose.landmarks_count; ++index) {
    const MpNormalizedLandmark& landmark = first_pose.landmarks[index];
    if (!LandmarkPassesGate(landmark, state)) {
      continue;
    }
    const float x = std::clamp(landmark.x, 0.0F, 1.0F);
    const float y = std::clamp(landmark.y, 0.0F, 1.0F);
    x_min = std::min(x_min, x);
    y_min = std::min(y_min, y);
    x_max = std::max(x_max, x);
    y_max = std::max(y_max, y);
    confidence_sum += LandmarkConfidence(landmark);
    ++accepted_count;
  }

  if (accepted_count < 3 || x_max <= x_min || y_max <= y_min) {
    return;
  }

  result.detected = 1;
  result.confidence = confidence_sum / static_cast<float>(accepted_count);
  result.normalized_x_min = x_min;
  result.normalized_y_min = y_min;
  result.normalized_x_max = x_max;
  result.normalized_y_max = y_max;
  result.bbox_center_x_ratio = (x_min + x_max) * 0.5F;
  result.bbox_area_ratio = (x_max - x_min) * (y_max - y_min);
}

}  // namespace

extern "C" WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API void*
winfaceunlock_mediapipe_pose_create(
    const char* model_path,
    WinFaceUnlockMediaPipeOptions options) {
  if (model_path == nullptr || model_path[0] == '\0') {
    return nullptr;
  }

  auto state = std::make_unique<ProviderState>();
  MpFaceLandmarkerOptions landmarker_options = {};
  landmarker_options.base_options.model_asset_path = model_path;
  landmarker_options.base_options.delegate = MP_DELEGATE_CPU;
  landmarker_options.base_options.host_environment = MP_HOST_ENVIRONMENT_UNKNOWN;
  landmarker_options.base_options.host_system = MP_HOST_SYSTEM_WINDOWS;
  landmarker_options.running_mode = ToMediaPipeRunningMode(options.running_mode);
  landmarker_options.num_faces = 1;
  landmarker_options.output_face_blendshapes = options.output_face_blendshapes != 0;
  landmarker_options.output_facial_transformation_matrixes =
      options.output_facial_transformation_matrixes != 0;

  char* error_message = nullptr;
  const MpStatus status =
      MpFaceLandmarkerCreate(&landmarker_options, &state->landmarker, &error_message);
  FreeError(error_message);
  if (status != kMpOk || state->landmarker == nullptr) {
    return nullptr;
  }

  return state.release();
}

extern "C" WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API void
winfaceunlock_mediapipe_pose_destroy(void* provider) {
  if (provider == nullptr) {
    return;
  }

  auto* state = static_cast<ProviderState*>(provider);
  if (state->landmarker != nullptr) {
    char* error_message = nullptr;
    MpFaceLandmarkerClose(state->landmarker, &error_message);
    FreeError(error_message);
    state->landmarker = nullptr;
  }
  delete state;
}

extern "C" WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API int
winfaceunlock_mediapipe_pose_estimate(
    void* provider,
    const WinFaceUnlockMediaPipeFrameRequest* request,
    WinFaceUnlockMediaPipePoseResult* result) {
  if (provider == nullptr || request == nullptr || result == nullptr) {
    return 1;
  }

  auto* state = static_cast<ProviderState*>(provider);
  if (state->landmarker == nullptr) {
    return 2;
  }

  *result = WinFaceUnlockMediaPipePoseResult{};
  MpImagePtr image = CreateImage(*request);
  if (image == nullptr) {
    return 3;
  }

  MpFaceLandmarkerResult landmark_result = {};
  char* error_message = nullptr;
  MpStatus status = MpFaceLandmarkerDetectImage(
      state->landmarker,
      image,
      nullptr,
      &landmark_result,
      &error_message);

  if (status == kMpFailedPrecondition) {
    FreeError(error_message);
    error_message = nullptr;
    status = MpFaceLandmarkerDetectForVideo(
        state->landmarker,
        image,
        nullptr,
        state->next_video_timestamp_ms++,
        &landmark_result,
        &error_message);
  }

  FreeError(error_message);
  if (status != kMpOk) {
    MpImageFree(image);
    return 4;
  }

  ExtractPoseFromMatrix(landmark_result, *result);
  ExtractBlinkScores(landmark_result, *result);

  MpFaceLandmarkerCloseResult(&landmark_result);
  MpImageFree(image);
  return 0;
}

extern "C" WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API void*
winfaceunlock_mediapipe_presence_pose_create(
    const char* model_path,
    WinFaceUnlockMediaPipePresencePoseOptions options) {
  if (model_path == nullptr || model_path[0] == '\0') {
    return nullptr;
  }

  auto state = std::make_unique<PresencePoseProviderState>();
  state->min_landmark_visibility = options.min_landmark_visibility;
  state->min_landmark_presence = options.min_landmark_presence;

  MpPoseLandmarkerOptions landmarker_options = {};
  landmarker_options.base_options.model_asset_path = model_path;
  landmarker_options.base_options.delegate = MP_DELEGATE_CPU;
  landmarker_options.base_options.host_environment = MP_HOST_ENVIRONMENT_UNKNOWN;
  landmarker_options.base_options.host_system = MP_HOST_SYSTEM_WINDOWS;
  landmarker_options.running_mode = ToMediaPipeRunningMode(options.running_mode);
  landmarker_options.num_poses = 1;
  landmarker_options.min_pose_detection_confidence = 0.5F;
  landmarker_options.min_pose_presence_confidence = 0.5F;
  landmarker_options.min_tracking_confidence = 0.5F;
  landmarker_options.output_segmentation_masks = false;

  char* error_message = nullptr;
  const MpStatus status =
      MpPoseLandmarkerCreate(&landmarker_options, &state->landmarker, &error_message);
  FreeError(error_message);
  if (status != kMpOk || state->landmarker == nullptr) {
    return nullptr;
  }

  return state.release();
}

extern "C" WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API void
winfaceunlock_mediapipe_presence_pose_destroy(void* provider) {
  if (provider == nullptr) {
    return;
  }

  auto* state = static_cast<PresencePoseProviderState*>(provider);
  if (state->landmarker != nullptr) {
    char* error_message = nullptr;
    MpPoseLandmarkerClose(state->landmarker, &error_message);
    FreeError(error_message);
    state->landmarker = nullptr;
  }
  delete state;
}

extern "C" WINFACEUNLOCK_MEDIAPIPE_BRIDGE_API int
winfaceunlock_mediapipe_presence_pose_estimate(
    void* provider,
    const WinFaceUnlockMediaPipeFrameRequest* request,
    WinFaceUnlockMediaPipePresencePoseResult* result) {
  if (provider == nullptr || request == nullptr || result == nullptr) {
    return 1;
  }

  auto* state = static_cast<PresencePoseProviderState*>(provider);
  if (state->landmarker == nullptr) {
    return 2;
  }

  *result = WinFaceUnlockMediaPipePresencePoseResult{};
  MpImagePtr image = CreateImage(*request);
  if (image == nullptr) {
    return 3;
  }

  MpPoseLandmarkerResult landmark_result = {};
  char* error_message = nullptr;
  MpStatus status = MpPoseLandmarkerDetectImage(
      state->landmarker,
      image,
      nullptr,
      &landmark_result,
      &error_message);

  if (status == kMpFailedPrecondition) {
    FreeError(error_message);
    error_message = nullptr;
    status = MpPoseLandmarkerDetectForVideo(
        state->landmarker,
        image,
        nullptr,
        state->next_video_timestamp_ms++,
        &landmark_result,
        &error_message);
  }

  FreeError(error_message);
  if (status != kMpOk) {
    MpImageFree(image);
    return 4;
  }

  ExtractPresencePoseResult(landmark_result, *state, *result);

  MpPoseLandmarkerCloseResult(&landmark_result);
  MpImageFree(image);
  return 0;
}
