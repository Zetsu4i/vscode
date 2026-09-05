/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Registry } from '../../../../platform/registry/common/platform.js';
import { IInstantiationService } from '../../../../platform/instantiation/common/instantiation.js';
import { IWorkbenchContribution, WorkbenchPhase, registerWorkbenchContribution2 } from '../../../common/contributions.js';
import { TerminalExtensions } from '../../../../platform/terminal/common/terminal.js';
import type { ITerminalBackendRegistry } from '../../../../platform/terminal/common/terminal.js';
import { ITerminalInstanceService } from '../../terminal/browser/terminal.js';
import { TauriTerminalBackend } from './tauriTerminalBackend.js';
import { TauriBridge } from '../common/tauriBridge.js';
import { registerSingleton, InstantiationType } from '../../../../platform/instantiation/common/extensions.js';
import { IFileDialogService } from '../../../../platform/dialogs/common/dialogs.js';
import { TauriFileDialogService } from './tauriFileDialogService.js';

/**
 * Registers the Tauri terminal backend when the workbench runs inside the
 * VSTauri shell. In a plain browser build of vscode-web this contribution is
 * a no-op, keeping upstream behavior fully intact.
 */
export class TauriTerminalBackendContribution implements IWorkbenchContribution {
	static ID = 'tauriTerminalBackend';

	constructor(
		@IInstantiationService instantiationService: IInstantiationService,
		@ITerminalInstanceService terminalInstanceService: ITerminalInstanceService
	) {
		const bridge = TauriBridge.get();
		if (bridge) {
			const backend = instantiationService.createInstance(TauriTerminalBackend, bridge);
			Registry.as<ITerminalBackendRegistry>(TerminalExtensions.Backend).registerTerminalBackend(backend);
			terminalInstanceService.didRegisterBackend(backend);
		}
	}
}

registerWorkbenchContribution2(TauriTerminalBackendContribution.ID, TauriTerminalBackendContribution, WorkbenchPhase.AfterRestored);

// ---------------------------------------------------------------------------
// File dialogs (tauriFileDialogService.ts): the workbench startup applies the
// global singleton registry AFTER BrowserMain's serviceCollection.set calls
// (src/vs/workbench/browser/workbench.ts, "All Contributed Services"), so an
// override registered only via serviceCollection.set is overwritten by
// upstream's browser FileDialogService - which for the `file` scheme throws
// "Can't open folders..." and saves through the File System Access API.
// Registering here makes ours the LAST descriptor for the id: this module
// imports tauriFileDialogService.js, which transitively evaluates upstream's
// registerSingleton(IFileDialogService, FileDialogService) first, so our push
// lands after it and the effective service is ours.
// ---------------------------------------------------------------------------
registerSingleton(IFileDialogService, TauriFileDialogService, InstantiationType.Delayed);
