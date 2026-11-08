export const isIos =
  typeof navigator !== "undefined" && /iPhone|iPad|iPod/i.test(navigator.userAgent);

export const isAndroid =
  typeof navigator !== "undefined" && /Android/i.test(navigator.userAgent);

export const isWindows =
  typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent);

export const isMac =
  typeof navigator !== "undefined" &&
  !isIos &&
  /Mac OS X|Macintosh/i.test(navigator.userAgent);

export const isDesktop = isWindows || isMac;
