const errorCopy = {
  archiveSession: 'The session could not be archived. Try again.',
  bridgeEvent:
    'This session could not be updated. Select it again to reconnect.',
  closePi: 'The Pi process could not be closed. Restart Tau and try again.',
  effortChange:
    'The thinking effort could not be changed. Reopen the session and try again.',
  extensionFailure:
    'A Pi extension failed. Review the extension setup and try again.',
  extensionResponse: 'The response could not be sent. Try again.',
  folderPicker: 'The folder picker could not be opened. Try again.',
  importProject:
    'The project could not be added. Choose the folder again or try another folder.',
  importRemoteProject:
    'The remote project could not be added. Choose the directory again.',
  loadWorkspace:
    'Your projects could not be loaded. Restart Tau and try again.',
  messageSend:
    'The message could not be sent. Reopen the session and try again.',
  modelChange:
    'The model could not be changed. Reopen the session and try again.',
  piStart: 'Pi could not be started. Restart Tau and try again.',
  projectOrder: 'The project order could not be saved. Try again.',
  remoteConnection:
    'Could not connect to the remote host. Check the SSH connection and try again.',
  remoteDirectory:
    'That remote directory could not be opened. Choose another directory or check its permissions.',
  remoteSessionUnavailable:
    'This remote session is no longer available. Choose another session or start a new one.',
  removeProject: 'The project could not be removed. Try again.',
  restoreSession: 'The session could not be restored. Try again.',
  sessionRefresh:
    'This session could not be refreshed. Select it again to reconnect.',
  sessionRegistration:
    'This session could not be saved. Continue here, then try reopening it.',
  sessionRename:
    'The session could not be renamed. Choose another name and try again.',
  settingConfirmation: 'The setting could not be confirmed. Try again.',
  sidebarChange: 'The sidebar change could not be saved. Try again.',
  stopWork:
    'The current work could not be stopped. Wait for it to finish or reopen the session.',
  unsavedSession:
    'The previous session is unavailable. Enter a message to continue in this new session.',
  workspaceSelection: 'This selection could not be saved. Select it again.',
} as const;

function rpcFailureCopy(command: string): string {
  switch (command) {
    case 'abort':
      return errorCopy.stopWork;
    case 'extension_ui_response':
      return errorCopy.extensionResponse;
    case 'prompt':
      return errorCopy.messageSend;
    case 'set_model':
      return errorCopy.modelChange;
    case 'set_session_name':
      return errorCopy.sessionRename;
    case 'set_thinking_level':
      return errorCopy.effortChange;
    default:
      return errorCopy.sessionRefresh;
  }
}

export { errorCopy, rpcFailureCopy };
