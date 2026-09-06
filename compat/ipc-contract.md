# VSTauri IPC Contract

Generated from the pristine upstream tree (v1.138.0) by `build/ipc-contract/extract-ipc-contract.mjs`.
Regenerate with `node build/ipc-contract/extract-ipc-contract.mjs` and commit the result together with any upstream merge.

## Summary

- Files scanned: **8824**
- Plain `vscode:` channels (product surface): **49** (+2 test-only)
- Protocol service channels (product surface): **56** (+5 test-only)
- Mountain channels answered in Rust: **8**
- Dynamic channel names (need manual tracing): **113**

## Plain channels (renderer <-> main, `ipcRenderer` surface bridged by the Wind shim)

| Channel | Kinds | Test-only | Main handler | Renderer calls |
| --- | --- | --- | --- | --- |
| `vscode:accessibilitySupportChanged` | renderer:on | no | 0 | 1 |
| `vscode:addRemoveFolders` | renderer:on | no | 0 | 1 |
| `vscode:browserView:areaPicked` | renderer:send | no | 0 | 1 |
| `vscode:browserView:areaPickStopped` | renderer:send | no | 0 | 1 |
| `vscode:browserView:elementCommentRemoved` | renderer:send | no | 0 | 1 |
| `vscode:browserView:elementPicked` | renderer:send | no | 0 | 1 |
| `vscode:browserView:elementPickStopped` | renderer:send | no | 0 | 1 |
| `vscode:browserView:hideHighlight` | renderer:on | no | 0 | 1 |
| `vscode:browserView:highlightElement` | renderer:on | no | 0 | 1 |
| `vscode:browserView:keydown` | renderer:send | no | 0 | 1 |
| `vscode:browserView:preloadReady` | renderer:send | no | 0 | 1 |
| `vscode:browserView:setElementComments` | renderer:on | no | 0 | 1 |
| `vscode:browserView:setLocalizedStrings` | renderer:on | no | 0 | 1 |
| `vscode:browserView:setTheme` | renderer:on | no | 0 | 1 |
| `vscode:browserView:showElementComment` | renderer:on | no | 0 | 1 |
| `vscode:browserView:startAreaPicker` | renderer:on | no | 0 | 1 |
| `vscode:browserView:startElementPicker` | renderer:on | no | 0 | 1 |
| `vscode:browserView:stopAreaPicker` | renderer:on | no | 0 | 1 |
| `vscode:browserView:stopElementPicker` | renderer:on | no | 0 | 1 |
| `vscode:configureAllowedUNCHost` | renderer:on | no | 0 | 1 |
| `vscode:createAgentHostMessageChannel` | main:on, main:removeListener | no | 2 | 0 |
| `vscode:createMessageChannel` | main:push | no | 1 | 0 |
| `vscode:createPtyHostMessageChannel` | main:on, main:removeHandler | no | 2 | 0 |
| `vscode:disablePromptForProtocolHandling` | renderer:on | no | 0 | 1 |
| `vscode:enterFullScreen` | renderer:on | no | 0 | 1 |
| `vscode:fetchShellEnv` | main:handle, renderer:invoke | no | 1 | 1 |
| `vscode:getDiagnosticInfo` | renderer:on | no | 0 | 1 |
| `vscode:handleChatRequest` | renderer:on, renderer:removeListener | no | 0 | 2 |
| `vscode:hello` | renderer:send | no | 0 | 1 |
| `vscode:leaveFullScreen` | renderer:on | no | 0 | 1 |
| `vscode:notifyZoomLevel` | main:handle, renderer:invoke | no | 1 | 1 |
| `vscode:onBeforeUnload` | renderer:on | no | 0 | 1 |
| `vscode:onWillUnload` | renderer:on | no | 0 | 1 |
| `vscode:openChatSession` | renderer:on, renderer:removeListener | no | 0 | 2 |
| `vscode:openDevTools` | main:on, renderer:send | no | 1 | 2 |
| `vscode:openFiles` | renderer:on | no | 0 | 2 |
| `vscode:openProxyAuthenticationDialog` | renderer:on | no | 0 | 1 |
| `vscode:registerAuxiliaryWindow` | main:handle | no | 1 | 0 |
| `vscode:reloadWindow` | main:on, renderer:send | no | 1 | 2 |
| `vscode:reportSharedProcessCrash` | renderer:on | no | 0 | 1 |
| `vscode:runAction` | renderer:on | no | 0 | 1 |
| `vscode:runKeybinding` | renderer:on | no | 0 | 1 |
| `vscode:selectAgentsFolder` | renderer:on, renderer:removeListener | no | 0 | 2 |
| `vscode:showArgvParseWarning` | renderer:on | no | 0 | 1 |
| `vscode:showCredentialsError` | renderer:on | no | 0 | 1 |
| `vscode:showInfoMessage` | renderer:on | no | 0 | 1 |
| `vscode:showResolveShellEnvError` | renderer:on | no | 0 | 1 |
| `vscode:showTranslatedBuildWarning` | renderer:on | no | 0 | 1 |
| `vscode:toggleDevTools` | main:on, renderer:send | no | 1 | 2 |

## Protocol channels (`vscode:hello` / `vscode:message` binary frames -> Mountain router)

| Channel | Registered (main) | Consumed (renderer) | Mountain status | Commands known |
| --- | --- | --- | --- | --- |
| `agentHost` | no | yes | not-implemented | unknown (dynamic) |
| `browserView` | yes | yes | not-implemented | unknown (dynamic) |
| `browserViewGroup` | yes | yes | not-implemented | unknown (dynamic) |
| `checksum` | yes | no | not-implemented | unknown (dynamic) |
| `customEndpointTelemetry` | yes | no | not-implemented | unknown (dynamic) |
| `diagnostics` | yes | yes | not-implemented | unknown (dynamic) |
| `download` | yes | yes | not-implemented | unknown (dynamic) |
| `encryption` | yes | no | not-implemented | unknown (dynamic) |
| `extensionGalleryManifest` | yes | yes | not-implemented | unknown (dynamic) |
| `extensionhostdebugservice` | yes | no | not-implemented | unknown (dynamic) |
| `extensionRecommendationNotification` | yes | yes | not-implemented | unknown (dynamic) |
| `extensions` | yes | yes | not-implemented | 26 (server switch) |
| `extensionTipsService` | yes | yes | not-implemented | unknown (dynamic) |
| `externalTerminal` | yes | no | not-implemented | unknown (dynamic) |
| `fileManagedSettings` | yes | yes | not-implemented | unknown (dynamic) |
| `IUserDataSyncResourceProviderService` | yes | no | not-implemented | unknown (dynamic) |
| `keyboardLayout` | yes | yes | implemented(*) | 1 (ProxyChannel: getKeyboardLayoutData) |
| `languagePacks` | yes | no | not-implemented | unknown (dynamic) |
| `launch` | yes | yes | implemented(3 commands) | unknown (dynamic) |
| `localFilesystem` | yes | yes | implemented(*) | unknown (dynamic) |
| `localGit` | yes | no | not-implemented | unknown (dynamic) |
| `logger` | yes | yes | implemented(*) | 5 (server switch) |
| `mcpGalleryManifest` | yes | yes | not-implemented | unknown (dynamic) |
| `mcpManagement` | yes | yes | not-implemented | unknown (dynamic) |
| `menubar` | yes | no | not-implemented | unknown (dynamic) |
| `meteredConnection` | yes | yes | not-implemented | unknown (dynamic) |
| `nativeHost` | yes | yes | implemented(*) | 104 (ProxyChannel: getWindows, getWindowCount, getActiveWindowId, getActiveWindowPosition, getNativeWindowHandle, openWindow, ...) |
| `nativeManagedSettings` | yes | yes | not-implemented | unknown (dynamic) |
| `playwright` | yes | no | not-implemented | unknown (dynamic) |
| `policy` | yes | yes | not-implemented | 2 (server switch) |
| `process` | yes | no | not-implemented | 4 (ProxyChannel: resolveProcesses, getSystemStatus, getSystemInfo, getPerformanceInfo) |
| `profileStorageListener` | yes | yes | not-implemented | unknown (dynamic) |
| `remoteextensionsenvironment` | yes | no | not-implemented | unknown (dynamic) |
| `remoteTunnel` | yes | no | not-implemented | unknown (dynamic) |
| `request` | yes | no | not-implemented | unknown (dynamic) |
| `sandboxHelper` | yes | no | not-implemented | unknown (dynamic) |
| `sharedWebContentExtractor` | yes | no | not-implemented | unknown (dynamic) |
| `sign` | yes | yes | not-implemented | 3 (ProxyChannel: createNewMessage, validate, sign) |
| `storage` | yes | yes | implemented(*) | 8 (server switch) |
| `telemetry` | yes | no | not-implemented | unknown (dynamic) |
| `telemetryAppender` | yes | yes | not-implemented | unknown (dynamic) |
| `update` | yes | no | not-implemented | 7 (ProxyChannel: checkForUpdates, downloadUpdate, applyUpdate, quitAndInstall, isLatestVersion, _applySpecificUpdate, ...) |
| `url` | yes | yes | not-implemented | 3 (ProxyChannel: create, open, registerHandler) |
| `urlHandler` | yes | yes | not-implemented | unknown (dynamic) |
| `userDataAutoSync` | yes | yes | not-implemented | unknown (dynamic) |
| `userDataProfiles` | yes | yes | implemented(*) | 4 (server switch) |
| `userDataSync` | yes | no | not-implemented | unknown (dynamic) |
| `userDataSyncAccount` | yes | no | not-implemented | unknown (dynamic) |
| `userDataSyncMachines` | yes | no | not-implemented | unknown (dynamic) |
| `userDataSyncStoreManagement` | yes | no | not-implemented | unknown (dynamic) |
| `userDataSyncUtil` | yes | yes | not-implemented | unknown (dynamic) |
| `v8InspectProfiling` | yes | no | not-implemented | unknown (dynamic) |
| `watcher` | yes | yes | not-implemented | unknown (dynamic) |
| `webContentExtractor` | yes | no | not-implemented | unknown (dynamic) |
| `webview` | yes | yes | not-implemented | 3 (ProxyChannel: setIgnoreMenuShortcuts, findInFrame, stopFindInFrame) |
| `workspaces` | yes | yes | not-implemented | 9 (ProxyChannel: enterWorkspace, createUntitledWorkspace, deleteUntitledWorkspace, getWorkspaceIdentifier, addRecentlyOpened, removeRecentlyOpened, ...) |

## Mountain coverage detail

Channels the Rust backend answers today (whole-channel `*` handlers or per-command match arms):

- `keyboardLayout`
- `launch`
- `localFilesystem`
- `localPty`
- `logger`
- `nativeHost`
- `storage`
- `userDataProfiles`

## ProxyChannel service surfaces

### `encryption` (secret storage)

Methods (0): 

### `keyboardLayout` (keyboard layout)

Methods (1): `getKeyboardLayoutData`

Events: `onDidChangeKeyboardLayout`

### `menubar` (native menu bar)

Methods (0): 

### `nativeHost` (window/dialog/clipboard/process surface)

Methods (104): `getWindows`, `getWindowCount`, `getActiveWindowId`, `getActiveWindowPosition`, `getNativeWindowHandle`, `openWindow`, `openAgentsWindow`, `syncSystemWideKeybindings`, `isFullScreen`, `toggleFullScreen`, `getCursorScreenPoint`, `isMaximized`, `maximizeWindow`, `unmaximizeWindow`, `minimizeWindow`, `moveWindowTop`, `positionWindow`, `isWindowAlwaysOnTop`, `toggleWindowAlwaysOnTop`, `setWindowAlwaysOnTop`, `updateWindowControls`, `updateWindowAccentColor`, `setMinimumSize`, `saveWindowSplash`, `setBackgroundThrottling`, `focusWindow`, `showMessageBox`, `showSaveDialog`, `showOpenDialog`, `pickFileFolderAndOpen`, `pickFileAndOpen`, `pickFolderAndOpen`, `pickWorkspaceAndOpen`, `showItemInFolder`, `setRepresentedFilename`, `setDocumentEdited`, `setApplicationBadge`, `openExternal`, `moveItemToTrash`, `getMediaAccessStatus`, `isAdmin`, `writeElevated`, `isRunningUnderARM64Translation`, `getOSProperties`, `getOSStatistics`, `getOSVirtualMachineHint`, `getOSColorScheme`, `hasWSLFeatureInstalled`, `getScreenshot`, `uploadFileViaMobileApi`, `getProcessId`, `killProcess`, `triggerPaste`, `readClipboardText`, `writeClipboardText`, `readClipboardFindText`, `writeClipboardFindText`, `writeClipboardBuffer`, `readClipboardBuffer`, `hasClipboard`, `readImage`, `newWindowTab`, `showPreviousWindowTab`, `showNextWindowTab`, `moveWindowTabToNewWindow`, `mergeAllWindowTabs`, `toggleWindowTabsBar`, `updateTouchBar`, `installShellCommand`, `uninstallShellCommand`, `notifyReady`, `relaunch`, `reload`, `closeWindow`, `quit`, `exit`, `openDevTools`, `toggleDevTools`, `openGPUInfoWindow`, `openDevToolsWindow`, `openContentTracingWindow`, `stopTracing`, `profileRenderer`, `startTracing`, `resolveProxy`, `resolveProxyWithPackage`, `readProxyConfigWithPackage`, `lookupAuthorization`, `lookupKerberosAuthorization`, `loadCertificates`, `isPortFree`, `findFreePort`, `windowsGetStringRegKey`, `showToast`, `clearToast`, `clearToasts`, `createZipFile`, `getSystemIdleState`, `getSystemIdleTime`, `getCurrentThermalState`, `isOnBatteryPower`, `startPowerSaveBlocker`, `stopPowerSaveBlocker`, `isPowerSaveBlockerStarted`

Events: `onDidOpenMainWindow`, `onDidMaximizeWindow`, `onDidUnmaximizeWindow`, `onDidFocusMainWindow`, `onDidBlurMainWindow`, `onDidChangeWindowFullScreen`, `onDidChangeWindowAlwaysOnTop`, `onDidFocusMainOrAuxiliaryWindow`, `onDidBlurMainOrAuxiliaryWindow`, `onDidChangeDisplay`, `onDidSuspendOS`, `onDidResumeOS`, `onDidChangeOnBatteryPower`, `onDidChangeThermalState`, `onDidChangeSpeedLimit`, `onWillShutdownOS`, `onDidLockScreen`, `onDidUnlockScreen`, `onDidChangeColorScheme`, `onDidChangePassword`, `onDidTriggerWindowSystemContextMenu`

### `process` (process info)

Methods (4): `resolveProcesses`, `getSystemStatus`, `getSystemInfo`, `getPerformanceInfo`

### `sign` (signing)

Methods (3): `createNewMessage`, `validate`, `sign`

### `update` (updates)

Methods (7): `checkForUpdates`, `downloadUpdate`, `applyUpdate`, `quitAndInstall`, `isLatestVersion`, `_applySpecificUpdate`, `setInternalOrg`

Events: `onStateChange`

### `url` (deep links)

Methods (3): `create`, `open`, `registerHandler`

### `userDataProfiles` (profile CRUD)

Methods (9): `createNamedProfile`, `createTransientProfile`, `createProfile`, `updateProfile`, `removeProfile`, `setProfileForWorkspace`, `resetWorkspaces`, `cleanUp`, `cleanUpTransientProfiles`

Events: `onDidChangeProfiles`, `onDidResetWorkspaces`

### `webview` (webview lifecycle)

Methods (3): `setIgnoreMenuShortcuts`, `findInFrame`, `stopFindInFrame`

Events: `onFoundInFrame`

### `workspaces` (workspace dialogs)

Methods (9): `enterWorkspace`, `createUntitledWorkspace`, `deleteUntitledWorkspace`, `getWorkspaceIdentifier`, `addRecentlyOpened`, `removeRecentlyOpened`, `clearRecentlyOpened`, `getRecentlyOpened`, `getDirtyWorkspaces`

Events: `onDidChangeRecentlyOpened`

## Explicit server channel commands

### `extensions`

- `onInstallExtension`
- `onDidInstallExtensions`
- `onUninstallExtension`
- `onDidUninstallExtension`
- `onDidUpdateExtensionMetadata`
- `zip`
- `install`
- `installFromLocation`
- `installExtensionsFromProfile`
- `getManifest`
- `getTargetPlatform`
- `installFromGallery`
- `installGalleryExtensions`
- `uninstall`
- `uninstallExtensions`
- `getInstalled`
- `toggleApplicationScope`
- `copyExtensions`
- `updateMetadata`
- `resetPinnedStateForAllUserExtensions`
- `getExtensionsControlManifest`
- `download`
- `cleanUp`
- `getConfigBasedTips`
- `getImportantExecutableBasedTips`
- `getOtherExecutableBasedTips`

### `logger`

- `onDidChangeLoggers`
- `onDidChangeVisibility`
- `onDidChangeLogLevel`
- `setLogLevel`
- `getRegisteredLoggers`

### `policy`

- `onDidChange`
- `updatePolicyDefinitions`

### `storage`

- `onDidChangeStorage`
- `getItems`
- `getValue`
- `getFallbackApplicationStorageItems`
- `updateItems`
- `compareAndSwap`
- `optimize`
- `isUsed`

### `userDataProfiles`

- `onDidChangeProfiles`
- `createProfile`
- `updateProfile`
- `removeProfile`

