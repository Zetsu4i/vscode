import { monaco } from "../monaco";
import { useSettingsStore } from "./settingsStore";
import { applyTheme } from "../theme/themes";
import { useUiStore } from "../state/uiStore";

let activeEditor: monaco.editor.IStandaloneCodeEditor | null = null;

/** MonacoPane registers its (singleton) editor so appliers can reach it. */
export function registerEditor(ed: monaco.editor.IStandaloneCodeEditor): void {
  activeEditor = ed;
  applyEditorOptions(ed);
}

/** Push the editor.* settings into a Monaco editor instance. */
export function applyEditorOptions(ed: monaco.editor.IStandaloneCodeEditor): void {
  const s = useSettingsStore.getState();
  ed.updateOptions({
    fontSize: s.get<number>("editor.fontSize"),
    fontFamily: s.get<string>("editor.fontFamily"),
    fontLigatures: s.get<boolean>("editor.fontLigatures"),
    wordWrap: s.get<boolean>("editor.wordWrap") ? "on" : "off",
    minimap: { enabled: s.get<boolean>("editor.minimap.enabled") },
    tabSize: s.get<number>("editor.tabSize"),
    lineNumbers: s.get<string>("editor.lineNumbers") === "off" ? "off" : "on",
  });
}

function applyAll(): void {
  const s = useSettingsStore.getState();

  if (activeEditor) applyEditorOptions(activeEditor);

  // Theme: the settings file is the source of truth once the key exists.
  // Keep uiStore.themeId in sync (without re-writing the setting) so the
  // terminal and other themeId consumers follow along.
  if (s.hasKey("workbench.colorTheme")) {
    const id = s.get<string>("workbench.colorTheme");
    if (id && id !== useUiStore.getState().themeId) {
      applyTheme(id, true);
      useUiStore.setState({ themeId: id });
    }
  }
}

/**
 * Wire all settings appliers. Call once at workbench boot: every settings
 * change (store revision bump) re-applies editor options and the theme,
 * debounced to survive bursts.
 */
export function initSettingsAppliers(): void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  useSettingsStore.subscribe(() => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(applyAll, 80);
  });
}
