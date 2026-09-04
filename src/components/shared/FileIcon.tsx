import { useIconTheme } from "../../theme/icons";

/**
 * Renders the active icon theme's image for a file/folder name.
 * Returns nothing when the theme disables icons (Minimal).
 */
export default function FileIcon({
  name,
  isDir,
  expanded = false,
  className = "file-icon",
}: {
  name: string;
  isDir: boolean;
  expanded?: boolean;
  className?: string;
}) {
  const theme = useIconTheme();
  const icon = theme.getIcon(name, isDir, expanded);
  if (!icon) return null;
  return <img src={icon} className={className} alt="" draggable={false} />;
}
