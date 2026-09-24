# Teitunnel for Raycast

Share a local port at a public address, list and stop your shares, and run the Doctor,
through the [Teitunnel](https://teitunnel.teispace.com) app (free, on your own Cloudflare
account).

Needs the Teitunnel app running with **Settings ▸ Integrations ▸ Allow connections** on.
The extension reaches it over a local socket (a named pipe on Windows) that only you can
open, with a token from Teitunnel's data folder. Sharing and stopping ask in Teitunnel
first, unless you choose **Always Allow** for `raycast` there.

## Development

The control client in `src/control-client` is copied from `../control-client/src`; run
`npm run vendor` after changing it (`npm test` fails while the copy is stale).

```sh
npm install
npm test        # logic tests and the vendored-copy check
npm run build   # ray build: type check and bundle
npm run dev     # ray develop, with Raycast open
```
