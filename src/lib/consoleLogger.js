import { invoke } from '@tauri-apps/api/core';

const originalConsole = {
  log: console.log,
  error: console.error,
  warn: console.warn,
  info: console.info
};

const MAX_LOG_LENGTH = 500;

function truncateIfNeeded(str) {
  if (str.length > MAX_LOG_LENGTH) {
    return str.substring(0, MAX_LOG_LENGTH) + `... (truncated ${str.length - MAX_LOG_LENGTH} chars)`;
  }
  return str;
}

function logToBackend(level, ...args) {
  const message = args.map(arg => {
    if (typeof arg === 'object') {
      try {
        const json = JSON.stringify(arg);
        return truncateIfNeeded(json);
      } catch {
        return String(arg);
      }
    }
    return truncateIfNeeded(String(arg));
  }).join(' ');
  
  invoke('log_message', { level, message }).catch(err => {
    originalConsole.error('failed to log to backend:', err);
  });
}

export function setupLogging() {
  console.log = (...args) => {
    originalConsole.log(...args);
    logToBackend('INFO', ...args);
  };
  
  console.error = (...args) => {
    originalConsole.error(...args);
    logToBackend('ERROR', ...args);
  };
  
  console.warn = (...args) => {
    originalConsole.warn(...args);
    logToBackend('WARN', ...args);
  };
  
  console.info = (...args) => {
    originalConsole.info(...args);
    logToBackend('INFO', ...args);
  };
}
