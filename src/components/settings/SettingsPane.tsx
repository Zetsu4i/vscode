import { useEffect, useMemo, useState } from "react";
import { useSettingsStore, SettingsScope } from "../../settings/settingsStore";
import { SETTINGS, SETTING_CATEGORIES, SettingDef } from "../../settings/registry";
import { useWorkspaceStore } from "../../state/workspaceStore";

/**
 * Settings editor (a special tab, like VSCode's settings UI):
 * search across all settings, category navigation, User/Workspace scope
 * toggle, modified indicators and per-key reset. Every change is written
 * straight to the scope's settings.json and applied live by the appliers.
 */
export default function SettingsPane() {
  const scope = useSettingsStore((s) => s.scope);
  const revision = useSettingsStore((s) => s.revision);
  const root = useWorkspaceStore((s) => s.root);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<SettingDef["category"] | "All">("All");

  useEffect(() => {
    void useSettingsStore.getState().load();
  }, []);

  const store = useSettingsStore.getState();
  void revision; // re-render on every settings change

  const defs = useMemo(() => {
    const q = query.trim().toLowerCase();
    return SETTINGS.filter((d) => {
      if (category !== "All" && d.category !== category) return false;
      if (!q) return true;
      return (
        d.key.toLowerCase().includes(q) ||
        d.title.toLowerCase().includes(q) ||
        d.description.toLowerCase().includes(q) ||
        d.category.toLowerCase().includes(q)
      );
    });
  }, [query, category, revision]);

  const grouped = useMemo(() => {
    const map = new Map<SettingDef["category"], SettingDef[]>();
    for (const d of defs) {
      const list = map.get(d.category) ?? [];
      list.push(d);
      map.set(d.category, list);
    }
    return map;
  }, [defs]);

  const workspaceBlocked = scope === "workspace" && !root;

  const setScope = (s: SettingsScope) => {
    useSettingsStore.getState().setScope(s);
  };

  const renderControl = (d: SettingDef) => {
    const value = store.get<unknown>(d.key);
    const disabled = workspaceBlocked;
    const onChange = (v: unknown) => void useSettingsStore.getState().set(d.key, v);

    switch (d.type) {
      case "boolean":
        return (
          <label className={`settings-switch ${value ? "on" : ""} ${disabled ? "disabled" : ""}`}>
            <input
              type="checkbox"
              checked={Boolean(value)}
              disabled={disabled}
              onChange={(e) => onChange(e.target.checked)}
            />
            <span className="settings-switch-track" />
          </label>
        );
      case "number":
        return (
          <input
            className="settings-input num"
            type="number"
            value={Number(value)}
            min={d.min}
            max={d.max}
            disabled={disabled}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (!Number.isNaN(n)) onChange(n);
            }}
          />
        );
      case "enum":
        return (
          <select
            className="settings-select"
            value={String(value)}
            disabled={disabled}
            onChange={(e) => onChange(e.target.value)}
          >
            {d.options?.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        );
      default:
        return (
          <input
            className="settings-input"
            type="text"
            value={String(value ?? "")}
            disabled={disabled}
            onChange={(e) => onChange(e.target.value)}
          />
        );
    }
  };

  return (
    <div className="settings-pane">
      <div className="settings-header">
        <input
          className="settings-search"
          type="text"
          placeholder="Search settings"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="settings-scope">
          <button
            className={scope === "user" ? "active" : ""}
            onClick={() => setScope("user")}
            title="User settings apply to every window"
          >
            User
          </button>
          <button
            className={scope === "workspace" ? "active" : ""}
            onClick={() => root && setScope("workspace")}
            disabled={!root}
            title={root ? "Workspace settings are stored in .vscode/settings.json" : "Open a folder to edit workspace settings"}
          >
            Workspace
          </button>
        </div>
      </div>

      {workspaceBlocked && (
        <div className="settings-note">
          Open a folder to edit workspace settings — currently showing the
          Workspace scope with no effect.
        </div>
      )}

      <div className="settings-body">
        <div className="settings-nav">
          <button
            className={`settings-nav-item ${category === "All" ? "active" : ""}`}
            onClick={() => setCategory("All")}
          >
            All Settings
          </button>
          {SETTING_CATEGORIES.map((c) => (
            <button
              key={c}
              className={`settings-nav-item ${category === c ? "active" : ""}`}
              onClick={() => setCategory(c)}
            >
              {c}
            </button>
          ))}
        </div>

        <div className="settings-rows">
          {defs.length === 0 && (
            <div className="settings-empty">No settings match “{query}”.</div>
          )}
          {[...grouped.entries()].map(([cat, list]) => (
            <div key={cat} className="settings-category">
              <div className="settings-category-title">{cat}</div>
              {list.map((d) => {
                const modifiedIn = store.modifiedIn(d.key);
                const isModified = modifiedIn !== null;
                return (
                  <div key={d.key} className={`settings-row ${isModified ? "modified" : ""}`}>
                    <div className="settings-row-main">
                      <div className="settings-row-title">
                        <span>{d.title}</span>
                        {isModified && (
                          <button
                            className="settings-reset"
                            title={`Reset ${d.key} (remove from ${modifiedIn} settings)`}
                            onClick={() => void useSettingsStore.getState().reset(d.key)}
                          >
                            <i className="codicon codicon-discard" />
                          </button>
                        )}
                      </div>
                      <div className="settings-row-key">{d.key}</div>
                      <div className="settings-row-desc">{d.description}</div>
                      {isModified && (
                        <div className="settings-row-modified">
                          Modified in {modifiedIn === "workspace" ? "Workspace" : "User"}
                        </div>
                      )}
                    </div>
                    <div className="settings-row-control">{renderControl(d)}</div>
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
