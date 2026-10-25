import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

/** Классы Tailwind: условные склеиваются, конфликтующие — последний побеждает. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
