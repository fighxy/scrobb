// Иконки транспорта: простые контуры в квадрате 24×24, цвет — `currentColor`.

const paths = {
  play: "M8 5.5v13l10.5-6.5z",
  pause: "M7 5h3.5v14H7zM13.5 5H17v14h-3.5z",
  next: "M6 5.5v13l9-6.5zM16.5 5.5H19v13h-2.5z",
  previous: "M18 5.5v13l-9-6.5zM5 5.5h2.5v13H5z",
  repeat: "M7 7h9l-2-2 1.4-1.4L20 8l-4.6 4.4L14 11l2-2H7a2 2 0 0 0-2 2v1H3v-1a4 4 0 0 1 4-4zm10 10H8l2 2-1.4 1.4L4 16l4.6-4.4L10 13l-2 2h9a2 2 0 0 0 2-2v-1h2v1a4 4 0 0 1-4 4z",
  volume: "M4 9h4l5-4v14l-5-4H4zM16 8.5a5 5 0 0 1 0 7l-1.4-1.4a3 3 0 0 0 0-4.2z",
  plus: "M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6z",
  close: "M6.4 5 12 10.6 17.6 5 19 6.4 13.4 12 19 17.6 17.6 19 12 13.4 6.4 19 5 17.6 10.6 12 5 6.4z",
} as const;

export type IconName = keyof typeof paths;

export function Icon({ name, size = 20 }: { name: IconName; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" fill="currentColor">
      <path d={paths[name]} />
    </svg>
  );
}
