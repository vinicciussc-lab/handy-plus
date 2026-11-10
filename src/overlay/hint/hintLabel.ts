import { keycapLabel, type OSType } from "@/lib/utils/keyboard";

/**
 * The correction shortcut as the hint pill prints it: "ctrl+section" becomes
 * "Ctrl + \" on Windows, where handy-keys calls the ABNT2 key left of Z
 * "section". On a Mac keyboard "section" is the § key, so it stays as is.
 */
export const hintShortcutLabel = (binding: string, osType: OSType): string =>
  keycapLabel(
    binding
      .split("+")
      .map((part) =>
        osType !== "macos" && part.trim() === "section" ? "\\" : part,
      )
      .join("+"),
    osType,
  );
