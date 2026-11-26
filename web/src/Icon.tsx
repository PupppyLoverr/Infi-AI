import { SOLAR } from "./solar";

/** A Solar glyph (24x24 viewBox), drawn in currentColor. */
export function Icon({ name, size = 18, className, style }: { name: string; size?: number; className?: string; style?: React.CSSProperties }) {
  const body = SOLAR[name] ?? SOLAR.info;
  return (
    <svg className={className} style={style} width={size} height={size} viewBox="0 0 24 24"
      dangerouslySetInnerHTML={{ __html: body.replace(/currentColor/g, "currentColor") }} />
  );
}

/** Solyra's own macOS-style app icon for a role (bundled PNGs). */
export function AppIcon({ role, size = 48 }: { role: string; size?: number }) {
  const known = ["files", "browser", "terminal", "editor", "viewer", "calculator", "settings", "music", "mail", "notes", "photos", "monitor", "appstore", "launchpad", "downloads", "trash", "folder", "maps", "wallpaper", "overview"];
  const src = known.includes(role) ? `/icons/${role}.png` : `/icons/overview.png`;
  return <img src={src} width={size} height={size} draggable={false} style={{ display: "block" }} alt="" />;
}
