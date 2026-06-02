# Local Face Models

Place the OpenCV Zoo ONNX model files in this directory. Model weights are
project-local runtime assets and are intentionally excluded from Git.

The default Phase 4 path uses:

```text
face_detection_yunet_2023mar.onnx
face_recognition_sface_2021dec.onnx
```

Optional benchmark variants:

```text
face_detection_yunet_2023mar_int8.onnx
face_detection_yunet_2023mar_int8bq.onnx
face_recognition_sface_2021dec_int8.onnx
face_recognition_sface_2021dec_int8bq.onnx
```

Optional guided enrollment pose enhancement:

```text
face_landmarker.task
```

`face_landmarker.task` is only loaded when `diagnostics_cli` is built with
`--features mediapipe-pose` and guided enrollment uses
`--pose-provider mediapipe`. The Windows login service does not load it.

Optional silent RGB liveness model:

```text
minifasnet_v2.onnx
```

`minifasnet_v2.onnx` is the MiniFASNet-V2 ONNX conversion of
`minivision-ai/Silent-Face-Anti-Spoofing` and is loaded by `face_liveness`
for single-frame silent anti-spoof inference when present.

Model provenance:

```text
Source: https://huggingface.co/garciafido/minifasnet-v2-anti-spoofing-onnx
Upstream: https://github.com/minivision-ai/Silent-Face-Anti-Spoofing
License: Apache-2.0
Expected SHA-256: d7b3cd9ba8a7ceb13baa8c4720902e27ca3112eff52f926c08804af6b6eecc7b
Input: 80x80 BGR crop, raw 0-255 pixel values, NCHW
Classes: 2D fake, real, 3D fake
```

The default liveness debug command uses MiniFASNet as the primary liveness
provider when `models/minifasnet_v2.onnx` exists. MiniFASNet spoof scores reject
the authentication window by default. Use `--minifasnet-diagnostic-only` only
when collecting scores without enforcing rejection.

Optional person presence models:

```text
yolov8n.onnx
MobileNetSSD_deploy.caffemodel
MobileNetSSD_deploy.prototxt
```

`yolov8n.onnx` is the preferred person detector for `opencv-dnn-person`
presence tracking. The MobileNet-SSD Caffe files are retained as a diagnostic
baseline because they are faster but less reliable on the tested cameras.

Model weights are intentionally excluded from Git; keep the upstream license and
attribution in product distributions.
