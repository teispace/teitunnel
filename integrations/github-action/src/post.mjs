// The action's post step (runs even when the job failed): in share mode, stops the
// share (its route and DNS record go), deletes the job's tunnel and updates the comment.
// Snapshots stay up; the cleanup mode removes them when the pull request closes.

import { readInputs } from "./lib.mjs";
import { cliEnv, comment, paths } from "./main.mjs";
import { error, getState, log, mask, readEvent, run, warning } from "./runner.mjs";

const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

/** Asks the share to stop (it removes its route itself), then insists after `seconds`. */
async function stop(pid, seconds) {
  if (!pid || !alive(pid)) return;
  process.kill(pid, "SIGTERM");
  const deadline = Date.now() + seconds * 1000;
  while (alive(pid) && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  if (alive(pid)) {
    warning("The share didn't stop in time; stopping it.");
    process.kill(pid, "SIGKILL");
  }
}

export async function post() {
  if (getState("mode") !== "share") return;
  const inputs = readInputs(process.env);
  mask(inputs.token);
  const cli = getState("cli");
  const hostname = getState("hostname");
  const machine = getState("machine");
  const env = cliEnv(inputs, paths().data);
  await stop(Number.parseInt(getState("pid"), 10), 30);
  // On Windows the signal can't be caught, and a killed share leaves its route: stop
  // it here (nothing to do when it's already gone).
  const listed = await run(cli, ["shares", "--json"], env);
  if (listed.code === 0 && listed.stdout.includes(`"${hostname}"`)) {
    const stopped = await run(cli, ["shares", "--stop", hostname], env);
    if (stopped.code !== 0) warning(`Couldn't remove the route for ${hostname}.`);
  }
  if (machine) {
    const deleted = await run(cli, ["tunnel", "delete", machine, "--yes"], env);
    if (deleted.code !== 0) warning(`Couldn't delete the job's tunnel "${machine}".`);
  }
  const url = getState("url");
  if (url) await comment(inputs, readEvent(), hostname, "stopped", url);
  log("Preview stopped.");
}

post().catch((err) => {
  error(err.message);
  process.exitCode = 1;
});
