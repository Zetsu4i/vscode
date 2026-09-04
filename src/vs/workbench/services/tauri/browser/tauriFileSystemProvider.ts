// tauri: seam — file system provider backed by the Rust file service
// (src-tauri/src/services/files.rs), registered for scheme `file` when the
// workbench runs inside the Tauri shell (see src/vs/workbench/browser/web.main.ts).
//
// Mirrors the provider contract of IFileSystemProvider exactly
// (src/vs/platform/files/common/files.ts). Port of the disk file system
// provider's observable behavior for Phase 2 slice A: stat / readdir /
// readFile / writeFile / mkdir / rename / delete.
//
// Ledgered gaps (docs/tauri/parity/files.md + ROADMAP.md): file watching is a
// no-op until the Rust watcher service lands; `useTrash` deletes are rejected
// until the trash service lands.

import { Event } from '../../../../base/common/event.js';
import { Disposable, IDisposable } from '../../../../base/common/lifecycle.js';
import { URI } from '../../../../base/common/uri.js';
import {
	FilePermission,
	FileSystemProviderCapabilities,
	FileType,
	IFileChange,
	IFileDeleteOptions,
	IFileOverwriteOptions,
	IFileSystemProvider,
	IFileWriteOptions,
	IStat,
	IWatchOptions
} from '../../../../platform/files/common/files.js';

interface TauriGlobal {
	readonly core: {
		readonly invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
	};
}

function tauriGlobal(): TauriGlobal | undefined {
	const candidate = (globalThis as { __TAURI__?: Partial<TauriGlobal> }).__TAURI__;
	return candidate?.core?.invoke ? (candidate as TauriGlobal) : undefined;
}

/** Whether the workbench is executing inside the Tauri shell. */
export function isTauriRuntime(): boolean {
	return tauriGlobal() !== undefined;
}

/** Wire shape of FileStat returned by the Rust `fs_stat` command. */
interface IFileStatDTO {
	readonly fileType: number;
	readonly ctime: number;
	readonly mtime: number;
	readonly size: number;
	readonly permissions: number | null;
}

export class TauriFileSystemProvider implements IFileSystemProvider {

	readonly capabilities: FileSystemProviderCapabilities;
	readonly onDidChangeCapabilities = Event.None;
	readonly onDidChangeFile: Event<readonly IFileChange[]>;
	readonly onDidWatchError = Event.None;

	private readonly _invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;

	constructor() {
		const tauri = tauriGlobal();
		if (!tauri) {
			throw new Error('TauriFileSystemProvider requires the Tauri runtime (globalThis.__TAURI__)');
		}
		this._invoke = tauri.core.invoke;
		this.capabilities = FileSystemProviderCapabilities.FileReadWrite;
		// Parity gap: no change events until the Rust watcher service lands (Phase 2).
		this.onDidChangeFile = Event.None;
	}

	watch(resource: URI, opts: IWatchOptions): IDisposable {
		// Parity gap: watching is not wired yet (Phase 2 watcher service).
		return Disposable.None;
	}

	async stat(resource: URI): Promise<IStat> {
		const dto = await this._invoke('fs_stat', { path: resource.fsPath }) as IFileStatDTO;
		return {
			type: dto.fileType as FileType,
			ctime: dto.ctime,
			mtime: dto.mtime,
			size: dto.size,
			permissions: dto.permissions === null ? undefined : dto.permissions as FilePermission
		};
	}

	async readdir(resource: URI): Promise<[string, FileType][]> {
		const entries = await this._invoke('fs_readdir', { path: resource.fsPath }) as [string, number][];
		return entries.map(([name, fileType]) => [name, fileType as FileType]);
	}

	async readFile(resource: URI): Promise<Uint8Array> {
		const data = await this._invoke('fs_read_file', { path: resource.fsPath }) as number[];
		return Uint8Array.from(data);
	}

	async writeFile(resource: URI, content: Uint8Array, opts: IFileWriteOptions): Promise<void> {
		await this._invoke('fs_write_file', { path: resource.fsPath, contents: Array.from(content) });
	}

	async mkdir(resource: URI): Promise<void> {
		await this._invoke('fs_mkdir', { path: resource.fsPath });
	}

	async delete(resource: URI, opts: IFileDeleteOptions): Promise<void> {
		await this._invoke('fs_delete', { path: resource.fsPath, recursive: opts.recursive, useTrash: opts.useTrash });
	}

	async rename(from: URI, to: URI, opts: IFileOverwriteOptions): Promise<void> {
		await this._invoke('fs_rename', { from: from.fsPath, to: to.fsPath, overwrite: opts.overwrite });
	}
}
