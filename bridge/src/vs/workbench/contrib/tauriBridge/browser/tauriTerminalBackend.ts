/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Emitter, Event } from '../../../../base/common/event.js';
import type { PerformanceMark } from '../../../../base/common/performance.js';
import { ITerminalLogService } from '../../../../platform/terminal/common/terminal.js';
import type { IPtyHostController, IPtyHostLatencyMeasurement, IProcessProperty, IShellLaunchConfig, IShellLaunchConfigDto, ITerminalBackend, ITerminalChildProcess, ITerminalProcessOptions, ITerminalProfile, ITerminalsLayoutInfo, ITerminalsLayoutInfoById, TitleEventSource, TerminalIcon } from '../../../../platform/terminal/common/terminal.js';
import type { IProcessEnvironment, OperatingSystem } from '../../../../base/common/platform.js';
import type { IProcessDetails } from '../../../../platform/terminal/common/terminalProcess.js';
import { IWorkspaceContextService } from '../../../../platform/workspace/common/workspace.js';
import { IConfigurationResolverService } from '../../../services/configurationResolver/common/configurationResolver.js';
import { IHistoryService } from '../../../services/history/common/history.js';
import { IStatusbarService } from '../../../services/statusbar/browser/statusbar.js';
import { BaseTerminalBackend } from '../../terminal/browser/baseTerminalBackend.js';
import { TauriPty } from './tauriPty.js';
import type { TauriBridge } from '../common/tauriBridge.js';

/**
 * Minimal IPtyHostController — the backbone hosts the ptys directly, there is
 * no separate pty host process to supervise.
 */
class NoopPtyHostController implements IPtyHostController {
	readonly onPtyHostExit = Event.None;
	readonly onPtyHostStart = Event.None;
	readonly onPtyHostUnresponsive = Event.None;
	readonly onPtyHostResponsive = Event.None;
	readonly onPtyHostRequestResolveVariables = Event.None;
	restartPtyHost(): Promise<void> { return Promise.resolve(); }
	acceptPtyHostResolvedVariables(): Promise<void> { return Promise.resolve(); }
	getProfiles(): Promise<ITerminalProfile[]> { return Promise.resolve([]); }
}

interface ITerminalProfileDto {
	profileName: string;
	path: string;
	isDefault: boolean;
	args?: string[] | string;
	overrideName?: boolean;
	isAutoDetected?: boolean;
}

export class TauriTerminalBackend extends BaseTerminalBackend implements ITerminalBackend {

	private readonly _ptys = new Map<number, TauriPty>();
	private _nextId = 1;

	readonly remoteAuthority: string | undefined = undefined;

	private readonly _whenReady = Promise.resolve();
	get whenReady(): Promise<void> { return this._whenReady; }
	setReady(): void { /* ready immediately */ }

	private readonly _onDidRequestDetach = this._register(new Emitter<{ requestId: number; workspaceId: string; instanceId: number }>());
	readonly onDidRequestDetach = this._onDidRequestDetach.event;

	constructor(
		private readonly _bridge: TauriBridge,
		@ITerminalLogService logService: ITerminalLogService,
		@IHistoryService historyService: IHistoryService,
		@IConfigurationResolverService configurationResolverService: IConfigurationResolverService,
		@IStatusbarService statusBarService: IStatusbarService,
		@IWorkspaceContextService workspaceContextService: IWorkspaceContextService,
	) {
		super(new NoopPtyHostController(), logService, historyService, configurationResolverService, statusBarService, workspaceContextService);

		// wire backbone events to the ptys
		this._register(this._bridge.listen<{ id: number; data: string; trackCommit?: boolean }>('pty.data')(e => {
			this._ptys.get(e.id)?.handleData({ data: e.data, trackCommit: e.trackCommit ?? false });
		}));
		this._register(this._bridge.listen<{ id: number; pid: number; cwd: string }>('pty.ready')(e => {
			this._ptys.get(e.id)?.handleBridgeReady(e.pid, e.cwd);
		}));
		this._register(this._bridge.listen<{ id: number; exitCode?: number }>('pty.exit')(e => {
			const pty = this._ptys.get(e.id);
			if (pty) {
				this._ptys.delete(e.id);
				pty.handleBridgeExit(e.exitCode);
			}
		}));
		this._register(this._bridge.listen<{ id: number; type: string; value: unknown }>('pty.property')(e => {
			this._ptys.get(e.id)?.handleDidChangeProperty(e as unknown as IProcessProperty);
		}));

		this._onPtyHostConnected.fire();
	}

	get isResponsive(): boolean { return true; }

	async createProcess(
		shellLaunchConfig: IShellLaunchConfig,
		_cwd: string,
		cols: number,
		rows: number,
		_unicodeVersion: '6' | '11',
		_env: IProcessEnvironment,
		_options: ITerminalProcessOptions,
		shouldPersist: boolean
	): Promise<ITerminalChildProcess> {
		const id = this._nextId++;
		const dto: IShellLaunchConfigDto = {
			name: shellLaunchConfig.name,
			executable: shellLaunchConfig.executable,
			args: shellLaunchConfig.args,
			cwd: shellLaunchConfig.cwd,
			env: shellLaunchConfig.env,
			useShellEnvironment: shellLaunchConfig.useShellEnvironment,
			reconnectionProperties: shellLaunchConfig.reconnectionProperties,
			type: shellLaunchConfig.type,
			isFeatureTerminal: shellLaunchConfig.isFeatureTerminal,
			forceShellIntegration: shellLaunchConfig.forceShellIntegration,
			tabActions: shellLaunchConfig.tabActions,
			shellIntegrationEnvironmentReporting: shellLaunchConfig.shellIntegrationEnvironmentReporting,
		};
		await this._bridge.call('pty.create', id, dto, cols, rows, shouldPersist);
		const pty = new TauriPty(id, shouldPersist, this._bridge);
		this._ptys.set(id, pty);
		return pty;
	}

	async attachToProcess(_id: number): Promise<ITerminalChildProcess | undefined> {
		return undefined; // no persistence yet
	}

	async attachToRevivedProcess(_id: number): Promise<ITerminalChildProcess | undefined> {
		return undefined;
	}

	async listProcesses(): Promise<IProcessDetails[]> {
		return [];
	}

	async getLatency(): Promise<IPtyHostLatencyMeasurement[]> {
		return [{ label: 'vstauri', latency: 0 }];
	}

	async getDefaultSystemShell(osOverride?: OperatingSystem): Promise<string> {
		return this._bridge.call<string>('sys.defaultShell', osOverride ?? undefined);
	}

	async getProfiles(_profiles: unknown, _defaultProfile: unknown, includeDetectedProfiles?: boolean): Promise<ITerminalProfile[]> {
		const dtos = await this._bridge.call<ITerminalProfileDto[]>('sys.terminalProfiles', includeDetectedProfiles ?? false);
		return dtos as unknown as ITerminalProfile[];
	}

	async getWslPath(original: string, _direction: 'unix-to-win' | 'win-to-unix'): Promise<string> {
		return original;
	}

	async getEnvironment(): Promise<IProcessEnvironment> {
		return this._bridge.call<IProcessEnvironment>('sys.env');
	}

	async getShellEnvironment(): Promise<IProcessEnvironment | undefined> {
		return undefined;
	}

	async setTerminalLayoutInfo(_layoutInfo?: ITerminalsLayoutInfoById): Promise<void> { /* no persistence yet */ }

	async getTerminalLayoutInfo(): Promise<ITerminalsLayoutInfo | undefined> {
		return undefined;
	}

	async updateTitle(_id: number, _title: string, _titleSource: TitleEventSource): Promise<void> { /* titles flow via pty.property */ }

	async updateIcon(_id: number, _userInitiated: boolean, _icon: TerminalIcon, _color?: string): Promise<void> { /* not supported */ }

	async setNextCommandId(_id: number, _commandLine: string, _commandId: string): Promise<void> { /* not supported */ }

	async getPerformanceMarks(): Promise<PerformanceMark[]> {
		return [];
	}

	async reduceConnectionGraceTime(): Promise<void> { /* no connection */ }

	async requestDetachInstance(_workspaceId: string, _instanceId: number): Promise<IProcessDetails | undefined> {
		throw new Error('Detaching terminals is not supported by the VSTauri backbone');
	}

	async acceptDetachInstanceReply(_requestId: number, _persistentProcessId?: number): Promise<void> { /* not supported */ }

	async persistTerminalState(): Promise<void> { /* no persistence yet */ }

	restartPtyHost(): void { /* nothing to restart */ }

	async installAutoReply(_match: string, _reply: string): Promise<void> { /* not supported */ }

	async uninstallAllAutoReplies(): Promise<void> { /* not supported */ }
}
