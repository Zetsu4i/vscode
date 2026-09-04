import { useMemo, useState } from "react";
import { useSettingsStore, FIELD_OF } from "../../state/settingsStore";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { SETTINGS, getAt, type SettingDef } from "../../settings/schema";
import { listThemeOptions } from "../../theme";

type Scope = "user" | "workspace";

function ScopeSwitch({
  scope,
  setScope,
  disabled,
}: {
  scope: Scope;
  setScope: (s: Scope) => void;
  disabled: boolean;
}) {
  return (
    <div className="settings-scope" title={disabled ? "Open a folder to edit workspace settings" : undefined}>
      <button
        className={`scope-btn ${scope === "user" ? "on" : ""}`}
        onClick={() => setScope("user")}
      >
        User
      </button>
      <button
        className={`scope-btn ${scope === "workspace" ? "on" : ""}`}
        disabled={disabled}
        onClick={() => setScope("workspace")}
      >
        Workspace
      </button>
    </div>
  );
}

function BooleanControl({ def, value, scope }: { def: SettingDef; value: boolean; scope: Scope }) {
  const update = useSettingsStore((s) => s.update);
  return (
    <label className="settings-checkbox">
      <input
        type="checkbox"
        checked={value}
        onChange={(e) => void update(def.id, e.target.checked, scope)}
      />
      <span>{value ? "on" : "off"}</span>
    </label>
  );
}

function NumberControl({ def, value, scope }: { def: SettingDef; value: number; scope: Scope }) {
  const update = useSettingsStore((s) => s.update);
  const step = def.numeric?.step ?? 1;
  return (
    <div className="settings-number">
      <button
        className="settings-spin"
        onClick={() => void update(def.id, value - step, scope)}
        disabled={def.numeric ? value <= def.numeric.min : false}
      >
        −
      </button>
      <input
        type="number"
        value={value}
        min={def.numeric?.min}
        max={def.numeric?.max}
        onChange={(e) => {
          const n = Number(e.target.value);
          if (Number.isFinite(n)) void update(def.id, n, scope);
        }}
      />
      <button
        className="settings-spin"
        onClick={() => void update(def.id, value + step, scope)}
        disabled={def.numeric ? value >= def.numeric.max : false}
      >
        +
      </button>
    </div>
  );
}

function StringControl({ def, value, scope }: { def: SettingDef; value: string; scope: Scope }) {
  const update = useSettingsStore((s) => s.update);
  return (
    <input
      className="settings-text"
      type="text"
      value={value}
      onChange={(e) => void update(def.id, e.target.value, scope)}
    />
  );
}

function EnumControl({
  def,
  value,
  scope,
  options,
}: {
  def: SettingDef;
  value: string;
  scope: Scope;
  options: { value: string; label: string }[];
}) {
  const update = useSettingsStore((s) => s.update);
  return (
    <select
      className="settings-select"
      value={value}
      onChange={(e) => void update(def.id, e.target.value, scope)}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

function SettingRow({ def, scope }: { def: SettingDef; scope: Scope }) {
  const effective = useSettingsStore((s) => (s as unknown as Record<string, unknown>)[FIELD_OF[def.id] ?? ""]);
  const userValues = useSettingsStore((s) => s.userValues);
  const workspaceValues = useSettingsStore((s) => s.workspaceValues);
  const reset = useSettingsStore((s) => s.reset);

  const value = effective ?? def.default;
  const modifiedIn =
    getAt(workspaceValues, def.id) !== undefined
      ? "workspace"
      : getAt(userValues, def.id) !== undefined
        ? "user"
        : null;

  return (
    <div className="setting-row">
      <div className="setting-head">
        <span className="setting-title">{def.id}</span>
        {modifiedIn && (
          <span
            className={`setting-modified ${modifiedIn === "workspace" ? "in-workspace" : ""}`}
            title={`Modified in ${modifiedIn} settings`}
          >
            ●
          </span>
        )}
        {modifiedIn && value !== def.default && (
          <button
            className="setting-reset"
            title="Reset this setting"
            onClick={() => void reset(def.id)}
          >
            <i className="codicon codicon-discard" /> Reset
          </button>
        )}
      </div>
      <div className="setting-desc">{def.description}</div>
      <div className="setting-control">
        {def.type === "boolean" && (
          <BooleanControl def={def} value={Boolean(value)} scope={scope} />
        )}
        {def.type === "number" && (
          <NumberControl def={def} value={Number(value)} scope={scope} />
        )}
        {def.type === "string" && (
          <StringControl def={def} value={String(value)} scope={scope} />
        )}
        {def.type === "enum" && (
          <EnumControl
            def={def}
            value={String(value)}
            scope={scope}
            options={
              def.id === "workbench.colorTheme"
                ? listThemeOptions()
                : (def.enumValues ?? [])
            }
          />
        )}
        {def.type === "enum" && def.id === "files.autoSave" && (
          <span className="setting-hint">
            afterDelay saves {useSettingsStore.getState().autoSaveDelay}s after you stop typing
          </span>
        )}
      </div>
    </div>
  );
}

export default function SettingsEditor() {
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<Scope>("user");
  const root = useWorkspaceStore((s) => s.root);
  const loaded = useSettingsStore((s) => s.loaded);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = SETTINGS.filter((def) => {
      if (scope === "workspace" && def.scope !== "resource") return false;
      if (!q) return true;
      return (
        def.id.toLowerCase().includes(q) ||
        def.description.toLowerCase().includes(q) ||
        def.category.toLowerCase().includes(q)
      );
    });
    // group by category, keep category order from schema
    const groups: { category: string; defs: SettingDef[] }[] = [];
    for (const def of list) {
      const g = groups.find((x) => x.category === def.category);
      if (g) g.defs.push(def);
      else groups.push({ category: def.category, defs: [def] });
    }
    return groups;
  }, [query, scope]);

  const total = filtered.reduce((n, g) => n + g.defs.length, 0);

  return (
    <div className="settings-editor">
      <div className="settings-toolbar">
        <div className="settings-search">
          <i className="codicon codicon-search" />
          <input
            autoFocus
            type="text"
            placeholder="Search settings"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          {query && (
            <button className="settings-clear" onClick={() => setQuery("")}>
              <i className="codicon codicon-close" />
            </button>
          )}
        </div>
        <ScopeSwitch scope={scope} setScope={setScope} disabled={!root} />
      </div>

      {!loaded && <div className="settings-note">Loading settings…</div>}

      <div className="settings-list">
        {total === 0 && <div className="settings-note">No settings match “{query}”.</div>}
        {filtered.map((g) => (
          <div key={g.category} className="settings-category">
            <div className="settings-category-title">{g.category}</div>
            {g.defs.map((def) => (
              <SettingRow key={def.id} def={def} scope={scope} />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
