import { type ChildProcess, spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

/** How long the daemon may take to start, far past what it takes on a busy machine */
const STARTUP_MS = 120_000;
/** How long it has to stop before it is killed */
const STOP_MS = 10_000;

/**
 * Starts the daemon for the browser tests through serve.sh, on E2E_PORT or else a port it picks
 * itself, waits until it says it has started, and puts the port it listens on in E2E_PORT, where
 * the workers, which load the config again after this, find it. Gives back what stops it
 */
export default async function startDaemon(): Promise<() => Promise<void>> {
  const serve = spawn(fileURLToPath(new URL("./serve.sh", import.meta.url)), {
    env: { ...process.env, E2E_PORT: process.env.E2E_PORT ?? "0" },
    // its own process group, which a Ctrl-C at the terminal does not reach and the last resort
    // of a stop kills whole
    detached: true,
    // serve.sh stops the daemon once its standard input ends, which the runner's exit ends too
    stdio: ["pipe", "pipe", "inherit"]
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

/**
 * The port the daemon names once it has started, read from its log on standard output. A start
 * that fails says what the daemon printed until then
 */
const started = (serve: ChildProcess) =>
  new Promise<number>((resolve, reject) => {
    let port: number | null = null;
    // what it printed before it started, kept for a start that fails
    let printed: string[] | null = [];
    const fail = (why: string) => {
      if (printed === null) return;
      reject(new Error(`${why}:\n${printed.join("\n")}`));
      printed = null;
    };
    const timer = setTimeout(
      () => fail(`the daemon did not start within ${STARTUP_MS / 1000} s`),
      STARTUP_MS
    );
    // read to the end, as a pipe nobody reads would stop the daemon once full
    createInterface({ input: serve.stdout! }).on("line", (line) => {
      if (printed === null) return;
      printed.push(line);
      const listening = /Listening for HTTP requests.*url=http:\/\/[^/\s]*:(\d+)/.exec(line);
      if (listening) port = Number(listening[1]);
      if (!line.includes("reseek started")) return;
      clearTimeout(timer);
      if (port === null) return fail("the daemon started without naming its port");
      printed = null;
      resolve(port);
    });
    // after it exited and every line it printed was read
    serve.on("close", (code, signal) => {
      clearTimeout(timer);
      fail(`the daemon exited with ${signal ?? code} before it started`);
    });
  });

/** Ends serve.sh's standard input, on which it stops the daemon and removes its directory */
const stop = async (serve: ChildProcess) => {
  if (serve.exitCode !== null || serve.signalCode !== null) return;
  const exited = new Promise((done) => serve.once("exit", done));
  serve.stdin!.end();
  const late = setTimeout(() => process.kill(-serve.pid!, "SIGKILL"), STOP_MS);
  await exited;
  clearTimeout(late);
};
