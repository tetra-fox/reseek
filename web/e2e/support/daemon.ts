import { type ChildProcess, spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

/** How long the daemon may take to start, far past what it takes on a busy machine */
const STARTUP_MS = 120_000;
/** How long it has to stop on SIGTERM before it is killed */
const STOP_MS = 10_000;

/**
 * Starts the daemon for the browser tests through serve.sh, on E2E_PORT or else a port it picks
 * itself, waits until it says it has started, and puts the port it listens on in E2E_PORT, where
 * the workers, which load the config again after this, find it. Gives back what stops it
 */
export default async function startDaemon(): Promise<() => Promise<void>> {
  const serve = spawn(fileURLToPath(new URL("./serve.sh", import.meta.url)), {
    env: { ...process.env, E2E_PORT: process.env.E2E_PORT ?? "0" },
    // its own process group, so a stop reaches the daemon serve.sh waits on
    detached: true,
    stdio: ["ignore", "pipe", "inherit"]
  });
  try {
    process.env.E2E_PORT = String(await started(serve));
  } catch (error) {
    await stop(serve);
    throw error;
  }
  process.stderr.write(
    `the daemon for the browser tests listens on port ${process.env.E2E_PORT}\n`
  );
  return () => stop(serve);
}

/** The port the daemon names once it has started, read from its log on standard output */
const started = (serve: ChildProcess) =>
  new Promise<number>((resolve, reject) => {
    let port: number | null = null;
    const timer = setTimeout(
      () => reject(new Error(`the daemon did not start within ${STARTUP_MS / 1000} s`)),
      STARTUP_MS
    );
    // read to the end, as a pipe nobody reads would stop the daemon once full
    createInterface({ input: serve.stdout! }).on("line", (line) => {
      const listening = /Listening for HTTP requests.*url=http:\/\/[^/\s]*:(\d+)/.exec(line);
      if (listening) port = Number(listening[1]);
      if (line.includes("reseek started")) {
        clearTimeout(timer);
        if (port === null) reject(new Error("the daemon started without naming its port"));
        else resolve(port);
      }
    });
    serve.on("exit", (code, signal) => {
      clearTimeout(timer);
      reject(new Error(`the daemon exited with ${signal ?? code} before it started`));
    });
  });

/** Stops serve.sh and the daemon with SIGTERM, which lets serve.sh remove its directory */
const stop = async (serve: ChildProcess) => {
  if (serve.exitCode !== null || serve.signalCode !== null) return;
  const exited = new Promise((done) => serve.once("exit", done));
  process.kill(-serve.pid!, "SIGTERM");
  const late = setTimeout(() => process.kill(-serve.pid!, "SIGKILL"), STOP_MS);
  await exited;
  clearTimeout(late);
};
