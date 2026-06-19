const FACE_TEMPLATES_CHANGED_EVENT = 'winfaceunlock:face-templates-changed';

export function notifyFaceTemplatesChanged() {
  window.dispatchEvent(new Event(FACE_TEMPLATES_CHANGED_EVENT));
}

export function subscribeFaceTemplatesChanged(handler: () => void) {
  window.addEventListener(FACE_TEMPLATES_CHANGED_EVENT, handler);
  return () => {
    window.removeEventListener(FACE_TEMPLATES_CHANGED_EVENT, handler);
  };
}
