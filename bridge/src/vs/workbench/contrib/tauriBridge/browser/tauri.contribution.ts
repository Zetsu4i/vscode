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
