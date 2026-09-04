/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Barrier } from '../../../../base/common/async.js';
import { ITerminalLaunchError, ITerminalLaunchResult, IProcessPropertyMap, ITerminalChildProcess, ProcessPropertyType } from '../../../../platform/terminal/common/terminal.js';
import { BasePty } from '../../terminal/common/basePty.js';
import { hasKey } from '../../../../base/common/types.js';
import type { TauriBridge } from '../common/tauriBridge.js';

/**
 * A terminal process spawned by the Rust backbone via a real PTY
 * (portable-pty / ConPTY / openpty). Modeled on `RemotePty`.
 */
export class TauriPty extends BasePty implements ITerminalChildProcess {

	private readonly _startBarrier: Barrier;

	constructor(
		id: number,
		shouldPersist: boolean,
		private readonly _bridge: TauriBridge
	) {
		super(id, shouldPersist);
		this._startBarrier = new Barrier();
	}

	async start(): Promise<ITerminalLaunchError | ITerminalLaunchResult | undefined> {
		const startResult = await this._bridge.call<ITerminalLaunchError | undefined>('pty.start', this.id);
		if (startResult && hasKey(startResult, { message: true })) {
			// An error occurred
			return startResult;
		}
		this._startBarrier.open();
		return undefined;
	}

	shutdown(immediate: boolean): void {
		this._startBarrier.wait().then(_ => {
			this._bridge.call('pty.shutdown', this.id, immediate).catch(() => { /* gone */ });
		});
	}

	input(data: string): void {
		if (this._inReplay) {
			return;
		}
		this._startBarrier.wait().then(_ => {
			this._bridge.call('pty.input', this.id, data).catch(() => { /* gone */ });
		});
	}

	sendSignal(signal: string): void {
		if (this._inReplay) {
			return;
		}
		this._startBarrier.wait().then(_ => {
			this._bridge.call('pty.sendSignal', this.id, signal).catch(() => { /* gone */ });
		});
	}

	async processBinary(data: string): Promise<void> {
		return this._bridge.call('pty.input', this.id, data);
	}

	resize(cols: number, rows: number, pixelWidth?: number, pixelHeight?: number): void {
		if (this._inReplay || (this._lastDimensions.cols === cols && this._lastDimensions.rows === rows)) {
			return;
		}
		this._startBarrier.wait().then(_ => {
			this._lastDimensions.cols = cols;
			this._lastDimensions.rows = rows;
			this._bridge.call('pty.resize', this.id, cols, rows, pixelWidth ?? 0, pixelHeight ?? 0).catch(() => { /* gone */ });
		});
	}

	async clearBuffer(): Promise<void> {
		return this._bridge.call('pty.clearBuffer', this.id);
	}

	acknowledgeDataEvent(charCount: number): void {
		if (this._inReplay) {
			return;
		}
		this._startBarrier.wait().then(_ => {
			this._bridge.call('pty.acknowledgeDataEvent', this.id, charCount).catch(() => { /* gone */ });
		});
	}

	async setUnicodeVersion(_version: '6' | '11'): Promise<void> {
		// xterm on the client handles unicode versions; the pty passes raw bytes
	}

	async refreshProperty<T extends ProcessPropertyType>(type: T): Promise<IProcessPropertyMap[T]> {
		if (type === ProcessPropertyType.Cwd) {
			const cwd = await this._bridge.call<string>('pty.cwd', this.id);
			this.handleDidChangeProperty({ type, value: cwd });
			return cwd as IProcessPropertyMap[T];
		}
		return this._properties[type];
	}

	async updateProperty<T extends ProcessPropertyType>(type: T, value: IProcessPropertyMap[T]): Promise<void> {
		this.handleDidChangeProperty({ type, value });
	}

	/** Called by the backend when the backbone reports the process exited. */
	handleBridgeExit(exitCode: number | undefined): void {
		this._startBarrier.open();
		this.handleExit(exitCode);
		this.dispose();
	}

	/** Called by the backend when the backbone reports the process is ready. */
	handleBridgeReady(pid: number, cwd: string): void {
		this.handleReady({ pid, cwd });
	}
}
