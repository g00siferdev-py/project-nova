/** Read a local image file as a data URL for `chat_send_message` IPC. */
export function readImageFileAsDataUrl(file: File): Promise<{ base64: string; mime: string }> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== "string") {
        reject(new Error("Could not read image file"));
        return;
      }
      const mime = file.type && file.type.startsWith("image/") ? file.type : "image/jpeg";
      resolve({ base64: result, mime });
    };
    reader.onerror = () => reject(reader.error ?? new Error("Could not read image file"));
    reader.readAsDataURL(file);
  });
}
