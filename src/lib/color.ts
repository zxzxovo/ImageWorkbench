export interface ParsedProjectColor {
  css: string;
  picker: string;
  alpha: number;
}

function parseRgbChannel(value: string): number | null {
  const trimmed = value.trim();
  const percentage = trimmed.endsWith("%");
  const numeric = Number(percentage ? trimmed.slice(0, -1) : trimmed);
  if (!Number.isFinite(numeric)) return null;
  if (percentage) {
    if (numeric < 0 || numeric > 100) return null;
    return Math.round((numeric / 100) * 255);
  }
  if (numeric < 0 || numeric > 255) return null;
  return Math.round(numeric);
}

function parseAlpha(value: string, allowByteValue = false): number | null {
  const trimmed = value.trim();
  const percentage = trimmed.endsWith("%");
  const numeric = Number(percentage ? trimmed.slice(0, -1) : trimmed);
  if (!Number.isFinite(numeric)) return null;
  if (percentage) return numeric >= 0 && numeric <= 100 ? numeric / 100 : null;
  if (numeric >= 0 && numeric <= 1) return numeric;
  if (allowByteValue && numeric <= 255) return numeric / 255;
  return null;
}

function toHexChannel(value: number): string {
  return value.toString(16).padStart(2, "0");
}

function createColor(red: number, green: number, blue: number, alpha = 1): ParsedProjectColor | null {
  if ([red, green, blue, alpha].some((value) => !Number.isFinite(value))) return null;
  if ([red, green, blue].some((value) => value < 0 || value > 255) || alpha < 0 || alpha > 1) return null;
  const channels = [red, green, blue].map(Math.round);
  const picker = `#${channels.map(toHexChannel).join("")}`;
  const normalizedAlpha = Math.round(alpha * 1000) / 1000;
  return {
    picker,
    alpha: normalizedAlpha,
    css: normalizedAlpha === 1
      ? picker
      : `rgba(${channels[0]}, ${channels[1]}, ${channels[2]}, ${normalizedAlpha})`,
  };
}

function parseHexColor(value: string): ParsedProjectColor | null {
  const match = value.match(/^#([0-9a-f]+)$/i);
  if (!match || ![3, 4, 6, 8].includes(match[1].length)) return null;
  const expanded = match[1].length <= 4
    ? [...match[1]].map((channel) => channel.repeat(2)).join("")
    : match[1];
  const red = Number.parseInt(expanded.slice(0, 2), 16);
  const green = Number.parseInt(expanded.slice(2, 4), 16);
  const blue = Number.parseInt(expanded.slice(4, 6), 16);
  const alpha = expanded.length === 8 ? Number.parseInt(expanded.slice(6, 8), 16) / 255 : 1;
  return createColor(red, green, blue, alpha);
}

function splitFunctionArguments(value: string): string[] | null {
  if (value.includes(",")) return value.split(",").map((part) => part.trim());
  const slashParts = value.split("/").map((part) => part.trim());
  if (slashParts.length > 2) return null;
  const channels = slashParts[0].split(/\s+/).filter(Boolean);
  return slashParts.length === 2 ? [...channels, slashParts[1]] : channels;
}

function parseRgbFunction(value: string): ParsedProjectColor | null {
  const match = value.match(/^rgba?\((.*)\)$/i);
  if (!match) return null;
  const parts = splitFunctionArguments(match[1]);
  if (!parts || (parts.length !== 3 && parts.length !== 4)) return null;
  const channels = parts.slice(0, 3).map(parseRgbChannel);
  if (channels.some((channel) => channel === null)) return null;
  const alpha = parts.length === 4 ? parseAlpha(parts[3]) : 1;
  if (alpha === null) return null;
  return createColor(channels[0]!, channels[1]!, channels[2]!, alpha);
}

function parseArgbFunction(value: string): ParsedProjectColor | null {
  const match = value.match(/^argb\((.*)\)$/i);
  if (!match) return null;
  const parts = splitFunctionArguments(match[1]);
  if (!parts || parts.length !== 4) return null;
  const alpha = parseAlpha(parts[0], true);
  const channels = parts.slice(1).map(parseRgbChannel);
  if (alpha === null || channels.some((channel) => channel === null)) return null;
  return createColor(channels[0]!, channels[1]!, channels[2]!, alpha);
}

function parseArgbHex(value: string): ParsedProjectColor | null {
  const match = value.match(/^0x([0-9a-f]{8})$/i);
  if (!match) return null;
  const alpha = Number.parseInt(match[1].slice(0, 2), 16) / 255;
  const red = Number.parseInt(match[1].slice(2, 4), 16);
  const green = Number.parseInt(match[1].slice(4, 6), 16);
  const blue = Number.parseInt(match[1].slice(6, 8), 16);
  return createColor(red, green, blue, alpha);
}

export function parseProjectColor(value: string): ParsedProjectColor | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  return parseHexColor(trimmed)
    ?? parseRgbFunction(trimmed)
    ?? parseArgbFunction(trimmed)
    ?? parseArgbHex(trimmed);
}
