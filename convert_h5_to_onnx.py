import os
import sys

# Force TensorFlow to use Legacy Keras (Keras 2) because GhostFaceNet is a Keras 2 model
os.environ["TF_USE_LEGACY_KERAS"] = "1"

import tf_keras as keras
import tensorflow as tf
import tf2onnx
import onnx
from onnxsim import simplify

h5_path = "models/GhostFaceNet_W1.3_S2_ArcFace.h5"
onnx_path = "models/ghostfacenet_v1_stride2.onnx"

if not os.path.exists(h5_path):
    print(f"Error: {h5_path} does not exist.")
    sys.exit(1)

print(f"Loading Keras model from {h5_path} using tf-keras...")
model = keras.models.load_model(h5_path, compile=False)

print("Converting to ONNX (opset 13)...")
spec = (tf.TensorSpec((None, 112, 112, 3), tf.float32, name="input"),)
model_proto, _ = tf2onnx.convert.from_keras(model, input_signature=spec, opset=13)

print("Simplifying ONNX model using onnxsim...")
try:
    model_simp, check = simplify(model_proto)
    if check:
        print("ONNX simplification successful!")
        onnx.save(model_simp, onnx_path)
    else:
        print("Simplification failed validation, saving unsimplified model...")
        onnx.save(model_proto, onnx_path)
except Exception as e:
    print(f"Error during simplification: {e}, saving unsimplified model...")
    onnx.save(model_proto, onnx_path)

print(f"Finished. Saved ONNX model to {onnx_path}")
