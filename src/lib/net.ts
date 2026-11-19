export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Request failed";
}

export function isLoopbackBind(host: string): boolean {
  const value = host.trim().toLowerCase();
  if (value === "::1" || value === "localhost") return true;
  const parts = value.split(".");
  if (parts.length !== 4) return false;
  if (parts.some((part) => !/^\d+$/.test(part))) return false;
  const numbers = parts.map((part) => Number(part));
  return numbers[0] === 127 && numbers.every((part) => part <= 255);
}
