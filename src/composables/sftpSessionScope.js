import { connectionCan } from "../utils/connectionCapabilities";

// SFTP 会话标识与失效守卫，browser 与 open-files 共用：
// 异步响应回来时据此丢弃已切换/已销毁会话的结果，避免串会话写入。
export function createSftpSessionScope(props) {
  let disposed = false;

  function currentSession(path) {
    if (!connectionCan(props.connection, "sftp") || !props.sessionId) return null;
    return {
      connectionId: props.connection.id,
      sessionId: props.sessionId,
      path,
    };
  }

  function isStaleSession(session) {
    return (
      disposed ||
      !session ||
      !session.connectionId ||
      !session.sessionId ||
      props.connection?.id !== session.connectionId ||
      props.sessionId !== session.sessionId
    );
  }

  function isDisposed() {
    return disposed;
  }

  function setDisposed(value) {
    disposed = Boolean(value);
  }

  return {
    currentSession,
    isDisposed,
    isStaleSession,
    setDisposed,
  };
}
