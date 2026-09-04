/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { IPickAndOpenOptions, IDialogService } from '../../../../platform/dialogs/common/dialogs.js';
import { URI } from '../../../../base/common/uri.js';
import { IWorkspaceContextService } from '../../../../platform/workspace/common/workspace.js';
import { IInstantiationService } from '../../../../platform/instantiation/common/instantiation.js';
import { IWorkspacesService } from '../../../../platform/workspaces/common/workspaces.js';
import { IConfigurationService } from '../../../../platform/configuration/common/configuration.js';
import { IFileService } from '../../../../platform/files/common/files.js';
import { IOpenerService } from '../../../../platform/opener/common/opener.js';
import { ILanguageService } from '../../../../editor/common/languages/language.js';
import { ILabelService } from '../../../../platform/label/common/label.js';
import { ICommandService } from '../../../../platform/commands/common/commands.js';
import { ICodeEditorService } from '../../../../editor/browser/services/codeEditorService.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { IHostService } from '../../../services/host/browser/host.js';
import { IHistoryService } from '../../../services/history/common/history.js';
import { IWorkbenchEnvironmentService } from '../../../services/environment/common/environmentService.js';
import { IPathService } from '../../../services/path/common/pathService.js';
import { IEditorService } from '../../../services/editor/common/editorService.js';
import { IRemoteAgentService } from '../../../services/remote/common/remoteAgentService.js';
import { FileDialogService } from '../../../services/dialogs/browser/fileDialogService.js';
import { TauriBridge } from '../common/tauriBridge.js';

/**
 * File dialogs backed by native OS dialogs from the Rust backbone (rfd).
 * Falls back to upstream browser behavior when the bridge is unavailable.
 *
 * The full constructor is redeclared (in AbstractFileDialogService's exact
 * parameter order) because vscode's DI does not walk the prototype chain for
 * parameter decorators.
 */
export class TauriFileDialogService extends FileDialogService {

        constructor(
                @IHostService protected override readonly hostService: IHostService,
                @IWorkspaceContextService contextService: IWorkspaceContextService,
                @IHistoryService historyService: IHistoryService,
                @IWorkbenchEnvironmentService environmentService: IWorkbenchEnvironmentService,
                @IInstantiationService instantiationService: IInstantiationService,
                @IConfigurationService configurationService: IConfigurationService,
                @IFileService fileService: IFileService,
                @IOpenerService openerService: IOpenerService,
                @IDialogService dialogService: IDialogService,
                @ILanguageService languageService: ILanguageService,
                @IWorkspacesService workspacesService: IWorkspacesService,
                @ILabelService labelService: ILabelService,
                @IPathService pathService: IPathService,
                @ICommandService commandService: ICommandService,
                @IEditorService editorService: IEditorService,
                @ICodeEditorService codeEditorService: ICodeEditorService,
                @ILogService logService: ILogService,
                @IRemoteAgentService remoteAgentService: IRemoteAgentService,
        ) {
                super(
                        hostService,
                        contextService,
                        historyService,
                        environmentService,
                        instantiationService,
                        configurationService,
                        fileService,
                        openerService,
                        dialogService,
                        languageService,
                        workspacesService,
                        labelService,
                        pathService,
                        commandService,
                        editorService,
                        codeEditorService,
                        logService,
                        remoteAgentService,
                );
        }

        private async _pickViaBridge(mode: 'folder' | 'file', options: IPickAndOpenOptions): Promise<URI | undefined> {
                const bridge = TauriBridge.get();
                if (!bridge) {
                        return undefined;
                }
                const defaultPath = options.defaultUri?.fsPath;
                const path = await bridge.call<string | null>('dialog.pick', mode, defaultPath);
                return path ? URI.file(path) : undefined;
        }

        override async pickFileFolderAndOpen(options: IPickAndOpenOptions): Promise<void> {
                if (!TauriBridge.get()) {
                        return super.pickFileFolderAndOpen(options);
                }
                const uri = await this._pickViaBridge('folder', options);
                if (uri) {
                        return this.hostService.openWindow([{ folderUri: uri }], { forceNewWindow: options.forceNewWindow, remoteAuthority: options.remoteAuthority });
                }
                // user cancelled the native dialog
        }

        override async pickFileAndOpen(options: IPickAndOpenOptions): Promise<void> {
                if (!TauriBridge.get()) {
                        return super.pickFileAndOpen(options);
                }
                const uri = await this._pickViaBridge('file', options);
                if (uri) {
                        return this.hostService.openWindow([{ fileUri: uri }], { forceNewWindow: options.forceNewWindow, remoteAuthority: options.remoteAuthority });
                }
        }

        override async pickFolderAndOpen(options: IPickAndOpenOptions): Promise<void> {
                return this.pickFileFolderAndOpen(options);
        }
}
