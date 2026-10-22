// SPDX-License-Identifier: Apache-2.0
// Images in notes (format.md §5.5, §6): loading them for the live preview,
// and file names for pasted or dropped files.

import { invoke } from "@tauri-apps/api/core";

const IMAGE_TYPES: Record<string, string> = {
  avif: "image/avif",
  gif: "image/gif",
  heic: "image/heic",
  jpeg: "image/jpeg",
  jpg: "image/jpeg",
  png: "image/png",
  svg: "image/svg+xml",
  webp: "image/webp",
};

const EXTENSION_OF_TYPE: Record<string, string> = {
  "image/gif": "gif",
  "image/heic": "heic",
  "image/jpeg": "jpg",
  "image/png": "png",
  "image/svg+xml": "svg",
  "image/webp": "webp",
};

/** A file's name, or one made from its type for a pasted image without one. */
export function fileName(file: File): string {
  return file.name || `image.${EXTENSION_OF_TYPE[file.type] ?? "bin"}`;
}

/**
 * Loads the images the open note links to, as object URLs, and releases them
 * when another note opens. `notePath` is the open note, or `null`.
 */
export function createImageCache(notePath: () => string | null): {
  load: (url: string) => Promise<string>;
  forget: () => void;
} {
  let images = new Map<string, Promise<string>>();
  return {
    load(url) {
      const path = notePath();
      if (path === null) return Promise.reject(new Error("no note open"));
      let image = images.get(url);
      if (!image) {
        const ext = /\.([a-z0-9]+)(?:[?#]|$)/i.exec(url)?.[1]?.toLowerCase() ?? "";
        image = invoke<ArrayBuffer>("read_linked_file", { notePath: path, link: url }).then(
          (bytes) => URL.createObjectURL(new Blob([bytes], { type: IMAGE_TYPES[ext] ?? "" })),
        );
        images.set(url, image);
      }
      return image;
    },
    forget() {
      for (const image of images.values()) {
        image.then(
          (url) => {
            URL.revokeObjectURL(url);
          },
          () => undefined,
        );
      }
      images = new Map();
    },
  };
}
