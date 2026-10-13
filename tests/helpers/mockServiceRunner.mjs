export const serviceRunnerState = {
  calls: [],
  pending: [],
};

export function resetServiceRunnerState() {
  serviceRunnerState.calls.length = 0;
  serviceRunnerState.pending.length = 0;
}

export function createServiceRunner() {
  return (command) => {
    serviceRunnerState.calls.push(command);
    return new Promise((resolve, reject) => {
      serviceRunnerState.pending.push({ command, reject, resolve });
    });
  };
}

export function resolveNext(command, value) {
  const index = serviceRunnerState.pending.findIndex((entry) => entry.command === command);
  if (index < 0) throw new Error(`no pending request for ${command}`);
  const [{ resolve }] = serviceRunnerState.pending.splice(index, 1);
  resolve(value);
}

export function rejectNext(command, error) {
  const index = serviceRunnerState.pending.findIndex((entry) => entry.command === command);
  if (index < 0) throw new Error(`no pending request for ${command}`);
  const [{ reject }] = serviceRunnerState.pending.splice(index, 1);
  reject(error);
}
