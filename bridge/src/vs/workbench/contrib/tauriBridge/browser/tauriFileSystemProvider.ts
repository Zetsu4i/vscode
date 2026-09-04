/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Emitter, Event } from '../../../../base/common/event.js';
import { Disposable, DisposableStore, IDisposable, toDisposable } from '../../../../base/common/lifecycle.js';
import { URI } from '../../../../base/common/uri.js';
import { FileChangeType, FileSystemProviderCapabilities, FileSystemProviderErrorCode, FilePermission, IFileChange, IFileDeleteOptions, IFileOverwriteOptions, IFileWriteOptions, IFileSystemProvider, IStat, IWatchOptions, FileType, createFileSystemProviderError } from '../../../../platform/files/common/files.js';

interface IStatDto {
        type: 'file' | 'dir' | 'symlink';
        ctime: number;
        mtime: number;
        size: number;
        readonly: boolean;
}

interface IReaddirEntry {
        name: string;
        type: 'file' | 'dir' | 'symlink';
}

interface IChangeDto {
        type: 'added' | 'updated' | 'deleted';
        path: string;
}

function dtoFileType(type: IStatDto['type']): FileType {
        switch (type) {
                case 'dir': return FileType.Directory;
                case 'symlink': return FileType.SymbolicLink;
                default: return FileType.File;
        }
}

function changeType(type: IChangeDto['type']): FileChangeType {
        switch (type) {
                case 'added': return FileChangeType.ADDED;
                case 'deleted': return FileChangeType.DELETED;
                default: return FileChangeType.UPDATED;
        }
}

/**
 * File system provider for the `file` scheme backed by the Rust backbone.
 * Every operation is a JSON-RPC call to the local Tauri server; change events
 * arrive over the bridge WebSocket (notify-based watcher server side).
 */
export class TauriFileSystemProvider extends Disposable implements IFileSystemProvider {

        readonly capabilities: FileSystemProviderCapabilities =
                FileSystemProviderCapabilities.FileReadWrite
                | FileSystemProviderCapabilities.FileFolderCopy
                | FileSystemProviderCapabilities.Trash;

        readonly onDidChangeCapabilities = Event.None;

        private readonly _onDidChangeFile = this._register(new Emitter<readonly IFileChange[]>());
        readonly onDidChangeFile = this._onDidChangeFile.event;

        private readonly _watchers = new Map<number, IDisposable>();
        private _nextWatchId = 1;

        constructor(private readonly _bridge: import('../common/tauriBridge.js').TauriBridge) {
                super();

                // file change events from the watcher
                this._register(_bridge.listen<{ changes: IChangeDto[] }>('fs.change')(changes => {
                        this._onDidChangeFile.fire(changes.changes.map(c => ({
                                type: changeType(c.type),
                                resource: URI.file(c.path)
                        })));
                }));

                this._register(_bridge.listen<{ watchId: number }>('fs.watch-end')(e => {
                        const w = this._watchers.get(e.watchId);
                        if (w) {
                                w.dispose();
                                this._watchers.delete(e.watchId);
                        }
                }));
        }

        watch(resource: URI, opts: IWatchOptions): IDisposable {
                const watchId = this._nextWatchId++;
                const store = new DisposableStore();
                // async start; server ignores unknown watch ids on unregister
                this._bridge.call<number>('fs.watch', resource.fsPath, opts.recursive, opts.excludes).then(id => {
                        if (store.isDisposed) {
                                this._bridge.call('fs.unwatch', id).catch(() => { /* server gone */ });
                                return;
                        }
                        // server may renumber; track by returned id
                        this._watchers.set(id, store);
                        watchIdRegistry.set(watchId, id);
                }).catch(() => { /* bridge gone: no events */ });
                watchIdRegistry.set(watchId, -1);

                store.add(toDisposable(() => {
                        const serverId = watchIdRegistry.get(watchId) ?? -1;
                        watchIdRegistry.delete(watchId);
                        if (serverId >= 0) {
                                this._bridge.call('fs.unwatch', serverId).catch(() => { /* ignore */ });
                        }
                }));
                return store;
        }

        async stat(resource: URI): Promise<IStat> {
                try {
                        const s = await this._bridge.call<IStatDto>('fs.stat', resource.fsPath);
                        return {
                                type: dtoFileType(s.type),
                                ctime: s.ctime,
                                mtime: s.mtime,
                                size: s.size,
                                permissions: s.readonly ? FilePermission.Readonly : undefined
                        };
                } catch (err) {
                        throw toProviderError(err);
                }
        }

        async mkdir(resource: URI): Promise<void> {
                try {
                        await this._bridge.call('fs.mkdir', resource.fsPath);
                } catch (err) {
                        throw toProviderError(err);
                }
        }

        async readdir(resource: URI): Promise<[string, FileType][]> {
                try {
                        const entries = await this._bridge.call<IReaddirEntry[]>('fs.readdir', resource.fsPath);
                        return entries.map(e => [e.name, dtoFileType(e.type)]);
                } catch (err) {
                        throw toProviderError(err);
                }
        }

        async delete(resource: URI, opts: IFileDeleteOptions): Promise<void> {
                try {
                        await this._bridge.call('fs.delete', resource.fsPath, opts.recursive, opts.useTrash);
                } catch (err) {
                        throw toProviderError(err);
                }
        }

        async rename(from: URI, to: URI, opts: IFileOverwriteOptions): Promise<void> {
                try {
                        await this._bridge.call('fs.rename', from.fsPath, to.fsPath, opts.overwrite);
                } catch (err) {
                        throw toProviderError(err);
                }
        }

        async copy(from: URI, to: URI, opts: IFileOverwriteOptions): Promise<void> {
                try {
                        await this._bridge.call('fs.copy', from.fsPath, to.fsPath, opts.overwrite);
                } catch (err) {
                        throw toProviderError(err);
                }
        }

        async readFile(resource: URI): Promise<Uint8Array> {
                try {
                        const b64 = await this._bridge.call<string>('fs.readFile', resource.fsPath);
                        const bin = atob(b64);
                        const bytes = new Uint8Array(bin.length);
                        for (let i = 0; i < bin.length; i++) {
                                bytes[i] = bin.charCodeAt(i);
                        }
                        return bytes;
                } catch (err) {
                        throw toProviderError(err);
                }
        }

        async writeFile(resource: URI, content: Uint8Array, opts: IFileWriteOptions): Promise<void> {
                try {
                        let binary = '';
                        const chunk = 0x8000;
                        for (let i = 0; i < content.length; i += chunk) {
                                binary += String.fromCharCode.apply(null, Array.from(content.subarray(i, Math.min(i + chunk, content.length))) as unknown as number[]);
                        }
                        await this._bridge.call('fs.writeFile', resource.fsPath, btoa(binary), opts.create, opts.overwrite);
                } catch (err) {
                        throw toProviderError(err);
                }
        }
}

// watch handle translation (client-local watch id -> server id)
const watchIdRegistry = new Map<number, number>();

function toProviderError(err: unknown): Error {
        const message = err instanceof Error ? err.message : String(err);
        let code = FileSystemProviderErrorCode.Unknown;
        if (/ENOENT|FileNotFound/i.test(message)) {
                code = FileSystemProviderErrorCode.FileNotFound;
        } else if (/EEXIST|already exists/i.test(message)) {
                code = FileSystemProviderErrorCode.FileExists;
        } else if (/EACCES|EPERM|denied|permission/i.test(message)) {
                code = FileSystemProviderErrorCode.NoPermissions;
        } else if (/EISDIR|is a directory/i.test(message)) {
                code = FileSystemProviderErrorCode.FileIsADirectory;
        } else if (/ENOTDIR|not a directory/i.test(message)) {
                code = FileSystemProviderErrorCode.FileNotADirectory;
        }
        return createFileSystemProviderError(message, code);
}
