const PROGRESS_SPEED_SAMPLE_SIZE = 30;

function getDisplayLength(value) {
  return String(value || "").replace(/[\u4e00-\u9fa5]/gu, "**").length;
}

function ellipsizeDisplayString(value, maxLength) {
  const max = maxLength - 3;
  let length = 0;
  let text = "";
  for (let index = 0; index < value.length; index += 1) {
    const charLength =
      value.charCodeAt(index) >= 0x4e00 && value.charCodeAt(index) <= 0x9fa5 ? 2 : 1;
    if (length + charLength > max) {
      return { text: `${text}...`, length: length + 3 };
    }
    length += charLength;
    text += value[index];
  }
  return { text: `${text}...`, length: length + 3 };
}

function formatTransferSize(size) {
  let value = Number(size || 0);
  const units = ["B", "KB", "MB", "GB", "TB"];
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  if (value >= 100) return `${value.toFixed(0)} ${units[unitIndex]}`;
  if (value >= 10) return `${value.toFixed(1)} ${units[unitIndex]}`;
  return `${value.toFixed(2)} ${units[unitIndex]}`;
}

function formatTransferTime(seconds) {
  let result = "";
  let value = Number(seconds || 0);
  if (value >= 3600) {
    result += `${Math.floor(value / 3600)}:`;
    value %= 3600;
  }
  const minutes = Math.floor(value / 60);
  result += minutes >= 10 ? String(minutes) : `0${minutes}`;
  result += ":";
  const roundedSeconds = Math.round(value % 60);
  result += roundedSeconds >= 10 ? String(roundedSeconds) : `0${roundedSeconds}`;
  return result;
}

export class TextProgressBar {
  constructor(writer, columns, tmuxPaneColumns = undefined) {
    this._writer = writer;
    this._lastUpdateTime = 0;
    this._tmuxPaneColumns = tmuxPaneColumns || 0;
    this._columns = this._tmuxPaneColumns > 1 ? this._tmuxPaneColumns - 1 : columns || 80;
    this._firstWrite = true;
    this._speedCount = 0;
    this._speedIndex = 0;
    this._timeSamples = new Array(PROGRESS_SPEED_SAMPLE_SIZE);
    this._stepSamples = new Array(PROGRESS_SPEED_SAMPLE_SIZE);
  }

  setTerminalColumns(columns) {
    this._columns = columns || 80;
    if (this._tmuxPaneColumns > 0) {
      this._tmuxPaneColumns = 0;
    }
  }

  onNum(count) {
    this._fileCount = count;
    this._fileIndex = 0;
  }

  onName(name) {
    this._fileName = name;
    this._fileIndex += 1;
    this._startTime = Date.now();
    this._timeSamples[0] = this._startTime;
    this._stepSamples[0] = 0;
    this._speedCount = 1;
    this._speedIndex = 1;
    this._fileStep = -1;
  }

  onSize(size) {
    this._fileSize = size;
  }

  onStep(step) {
    if (step <= this._fileStep) return;
    this._fileStep = step;
    this._showProgress();
  }

  hideCursor() {
    this._writer("\x1b[?25l");
  }

  showCursor() {
    this._writer("\x1b[?25h");
  }

  _showProgress() {
    const now = Date.now();
    if (now - this._lastUpdateTime < 200) return;
    this._lastUpdateTime = now;

    const percentage =
      this._fileSize === 0 ? "100%" : `${Math.round((this._fileStep * 100) / this._fileSize)}%`;
    const total = formatTransferSize(this._fileStep);
    const speed = this._getSpeed(now);
    let speedText = "--- B/s";
    let etaText = "--- ETA";
    if (speed > 0) {
      speedText = `${formatTransferSize(speed)}/s`;
      etaText = `${formatTransferTime(Math.round((this._fileSize - this._fileStep) / speed))} ETA`;
    }

    const progressText = this._getProgressText(percentage, total, speedText, etaText);
    if (this._firstWrite) {
      this._firstWrite = false;
      this._writer(progressText);
      return;
    }
    if (this._tmuxPaneColumns > 0) {
      this._writer(`\x1b[${this._columns}D${progressText}`);
    } else {
      this._writer(`\r${progressText}`);
    }
  }

  _getSpeed(now) {
    let speed;
    if (this._speedCount <= PROGRESS_SPEED_SAMPLE_SIZE) {
      this._speedCount += 1;
      speed = ((this._fileStep - this._stepSamples[0]) * 1000) / (now - this._timeSamples[0]);
    } else {
      speed =
        ((this._fileStep - this._stepSamples[this._speedIndex]) * 1000) /
        (now - this._timeSamples[this._speedIndex]);
    }
    this._timeSamples[this._speedIndex] = now;
    this._stepSamples[this._speedIndex] = this._fileStep;
    this._speedIndex += 1;
    if (this._speedIndex >= PROGRESS_SPEED_SAMPLE_SIZE) {
      this._speedIndex %= PROGRESS_SPEED_SAMPLE_SIZE;
    }
    return Number.isFinite(speed) ? speed : -1;
  }

  _getProgressText(percentage, total, speed, eta) {
    const barMinLength = 24;
    let left =
      this._fileCount > 1
        ? `(${this._fileIndex}/${this._fileCount}) ${this._fileName}`
        : this._fileName;
    let leftLength = getDisplayLength(left);
    let right = ` ${percentage} | ${total} | ${speed} | ${eta}`;

    progressLayout: {
      if (this._columns - leftLength - right.length >= barMinLength) break progressLayout;
      if (leftLength > 50) {
        const result = ellipsizeDisplayString(left, 50);
        left = result.text;
        leftLength = result.length;
      }
      if (this._columns - leftLength - right.length >= barMinLength) break progressLayout;
      if (leftLength > 40) {
        const result = ellipsizeDisplayString(left, 40);
        left = result.text;
        leftLength = result.length;
      }
      if (this._columns - leftLength - right.length >= barMinLength) break progressLayout;
      right = ` ${percentage} | ${speed} | ${eta}`;
      if (this._columns - leftLength - right.length >= barMinLength) break progressLayout;
      if (leftLength > 30) {
        const result = ellipsizeDisplayString(left, 30);
        left = result.text;
        leftLength = result.length;
      }
      if (this._columns - leftLength - right.length >= barMinLength) break progressLayout;
      right = ` ${percentage} | ${eta}`;
      if (this._columns - leftLength - right.length >= barMinLength) break progressLayout;
      right = ` ${percentage}`;
      if (this._columns - leftLength - right.length >= barMinLength) break progressLayout;
      if (leftLength > 20) {
        const result = ellipsizeDisplayString(left, 20);
        left = result.text;
        leftLength = result.length;
      }
      if (this._columns - leftLength - right.length >= barMinLength) break progressLayout;
      left = "";
      leftLength = 0;
    }

    let barLength = this._columns - right.length;
    if (leftLength > 0) {
      barLength -= leftLength + 1;
      left += " ";
    }
    return (left + this._getProgressBar(barLength) + right).trim();
  }

  _getProgressBar(length) {
    if (length < 12) return "";
    const total = length - 2;
    const complete =
      this._fileSize === 0 ? total : Math.round((total * this._fileStep) / this._fileSize);
    return `[\x1b[36m${"█".repeat(complete)}${"░".repeat(total - complete)}\x1b[0m]`;
  }

  onDone() {
    if (this._fileSize === 0) return;
    this._fileStep = this._fileSize;
    this._lastUpdateTime = 0;
    this._showProgress();
  }
}
