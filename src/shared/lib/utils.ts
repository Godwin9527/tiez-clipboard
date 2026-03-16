import type { Locale } from "../types";

// Helper function to generate a consistent color from a string based on theme
export const getTagColor = (tag: string, theme: string) => {
  let hash = 0;
  for (let i = 0; i < tag.length; i++) {
    hash = tag.charCodeAt(i) + ((hash << 5) - hash);
  }

  // Use a larger prime and multiple rotations to ensure strings that are similar
  // (like "tag1" and "tag2") produce very different hues.
  const hue = Math.abs((hash * 137.508 + (hash >> 3)) % 360);

  if (theme === "retro") {
    // Retro: Slightly desaturated, lower lightness for mechanical look
    return `hsl(${hue}, 60%, 40%)`;
  } else {
    // Modern: Vibrant for Mica/Acrylic
    return `hsl(${hue}, 80%, 55%)`;
  }
};

export const getConciseTime = (timestamp: number, _language: Locale) => {
  const d = new Date(timestamp);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
};
