# Changelog

## [0.4.0](https://github.com/teispace/teitunnel/compare/v0.3.1...v0.4.0) (2026-09-26)


### Features

* **access:** GitHub and Google sign-in for route logins ([#70](https://github.com/teispace/teitunnel/issues/70)) ([61b8da1](https://github.com/teispace/teitunnel/commit/61b8da1cbf55c47cf8b3d8c82577e9677acedaa5)), closes [#51](https://github.com/teispace/teitunnel/issues/51)
* **protection:** bypass Cloudflare's cache for a hostname ([#69](https://github.com/teispace/teitunnel/issues/69)) ([d91ddcf](https://github.com/teispace/teitunnel/commit/d91ddcf6125c73fb0b8dd4221289b13a02108bac))


### Bug Fixes

* **desktop:** bundle the tray library in the AppImage ([f9682c9](https://github.com/teispace/teitunnel/commit/f9682c9009038dff5f43464bc39a12327b12d5ed))


### Performance

* **engine:** create the DNS records of several routes in one call per zone ([#67](https://github.com/teispace/teitunnel/issues/67)) ([5ebdaff](https://github.com/teispace/teitunnel/commit/5ebdaff9f887cb3cda02ed924e32d720bf258e2c)), closes [#49](https://github.com/teispace/teitunnel/issues/49)

## [0.3.1](https://github.com/teispace/teitunnel/compare/v0.3.0...v0.3.1) (2026-09-26)


### Bug Fixes

* **desktop:** log a Settings window that failed to build ([#44](https://github.com/teispace/teitunnel/issues/44)) ([b6e87bd](https://github.com/teispace/teitunnel/commit/b6e87bd90c9d40a804c9b2ce34af77559cb7b4b8))
* **desktop:** Settings opens once on a double click, and again after a failed build ([#58](https://github.com/teispace/teitunnel/issues/58)) ([1a3eb4e](https://github.com/teispace/teitunnel/commit/1a3eb4e37582ed1a0364c1f21febdd6cde7752dc))

## [0.3.0](https://github.com/teispace/teitunnel/compare/v0.2.0...v0.3.0) (2026-09-26)


### Features

* **access:** let webhooks skip a route's login ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **analytics:** request rates, browsers and bots, and numbers for every share ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **backup:** schedules move with a backup ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **browser:** an extension that shares the local page you're on ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **cli:** folder shares go to the running app ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **cli:** teitunnel share --comments ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **doctor:** leftovers on hostnames nothing serves are found and cleaned up ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **fronts:** save an inbox's signing secret in the app ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **inspector:** breakpoints hold requests and answers to change, answer or drop ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **inspector:** filter, search and read WebSocket frames whole ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **mcp:** agents can test how an app copes through the inspector ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **mcp:** agents give dev servers local HTTPS addresses ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **mcp:** OAuth sign-in for shared MCP servers, each connection approved ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **mcp:** offline page and webhook inbox tools ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **mcp:** route_health reports uptime, response times and incidents ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **overview:** live dashboard with traffic, errors, uptime and recent requests ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **quick-share:** protect a Quick Share from its card ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **routes:** pause and schedule a route from the Routes view ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **sharing:** pause and resume a Quick Share from its card, the CLI and MCP ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **verify:** name links visitors can't follow, with the framework's fix ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))


### Bug Fixes

* **cli:** servers without the app remove shares left behind ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **core:** a hostname's service tokens go with its last route ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **core:** deleting a tunnel cleans up its hostnames' tokens, pauses and schedules ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **core:** keychain items are shared by every Teitunnel program on macOS ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **core:** removing a hostname's last route removes everything attached to it ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **desktop:** Settings opens on Windows ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **engine:** a rolled-back plan puts a verifying inbox back with its secret ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **engine:** deleting a tunnel removes its hostnames' Workers and edge rules ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **runtime:** connectors recover from sleep, network changes and being offline ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **sharing:** folder shares and new addresses work on the first try ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))


### Performance

* **core:** API calls stop reading the keychain each time ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **desktop:** commands that do I/O run off the main thread ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **desktop:** connector changes refresh the views; Cloudflare is polled less ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))
* **inspector:** 10,000 requests at 60 fps, measured on a production build ([a360f02](https://github.com/teispace/teitunnel/commit/a360f0212332d1a31412135c0625e8a4db23381f))

## [0.2.0](https://github.com/teispace/teitunnel/compare/v0.1.1...v0.2.0) (2026-09-25)


### Features

* M12 platform, current Cloudflare API and sign-in for 0.2.0 ([#34](https://github.com/teispace/teitunnel/issues/34)) ([145cb0c](https://github.com/teispace/teitunnel/commit/145cb0caa59fb8944fa21388ad9e778dbcd6beca))


### Bug Fixes

* docs, landing page and fixes for 0.2.0 ([#36](https://github.com/teispace/teitunnel/issues/36)) ([c38adac](https://github.com/teispace/teitunnel/commit/c38adac4a6838f585daf8a3a31e387262123b2c3))

## [0.1.1](https://github.com/teispace/teitunnel/compare/v0.1.0...v0.1.1) (2026-09-24)


### Bug Fixes

* **linux:** depend on the tray library; check every package on real systems before release ([#27](https://github.com/teispace/teitunnel/issues/27)) ([616ea35](https://github.com/teispace/teitunnel/commit/616ea355b70ad1b733e477eb269bc4c011b3ea63))

## [0.1.0](https://github.com/teispace/teitunnel/compare/v0.1.0...v0.1.0) (2026-09-23)


### Features

* **a11y:** WCAG AA under Increase Contrast, Reduce Motion for springs, Title Case buttons ([d16b643](https://github.com/teispace/teitunnel/commit/d16b6438367968ad4522c2d427ae126cd706e826))
* **accounts:** add the Access permission from where a login is needed ([bd2c918](https://github.com/teispace/teitunnel/commit/bd2c918b30cb572ba74a8a2bbe74decf61a01f0c))
* **accounts:** ask for Access in the token template ([47d88ad](https://github.com/teispace/teitunnel/commit/47d88aded65c92685e695357e072a15c5ef4e65c))
* **accounts:** connect Cloudflare, accounts settings and the Domains view ([a74c5b7](https://github.com/teispace/teitunnel/commit/a74c5b7721b472dd480ab678322608579290916e))
* **accounts:** connect from the Overview; filter long domain lists; M2 docs ([f4e4a08](https://github.com/teispace/teitunnel/commit/f4e4a08ad6649c0d4fee4573c3da489636888d87))
* **accounts:** keychain secret store and account service ([4bdb0e7](https://github.com/teispace/teitunnel/commit/4bdb0e73d4c259a2d91a780163203bb004b5ee2b))
* **accounts:** probe credential capabilities without side effects ([68ab7bf](https://github.com/teispace/teitunnel/commit/68ab7bf2e94ffa565b202543f23dc1f257a75958))
* **accounts:** token template link, domains and account commands ([80d3bb3](https://github.com/teispace/teitunnel/commit/80d3bb3e4f9210236ec3f18b457de4d3e013ab19))
* **activity:** before/after changes, step states, copy as commands, check again ([ee67489](https://github.com/teispace/teitunnel/commit/ee67489ec85c0c727177420f5acfa82e1e33a97a))
* **activity:** filter by text and show only problems ([1451246](https://github.com/teispace/teitunnel/commit/14512466dfec0f6dc5239955e6035ed050df822b))
* **activity:** timeline of changes with outcome and steps ([2e103f9](https://github.com/teispace/teitunnel/commit/2e103f92eb78ed5c6506404cff8f4bf7d1ba0e16))
* **always-on:** keep this Mac's connector running after quit, via launchd ([1c0b653](https://github.com/teispace/teitunnel/commit/1c0b653f0101e70d86f640873eab5fd578ed30c4))
* **app:** ask before quitting while routes run through the app ([208be63](https://github.com/teispace/teitunnel/commit/208be6320e0350d8839fd051cb86a930004b6fc5))
* **app:** open at login, hidden in the menu bar ([526958a](https://github.com/teispace/teitunnel/commit/526958a4bc3148b63b9c8caea35efc3a343ae527))
* **binary:** onboarding, cloudflared settings pane and update check ([12ffe01](https://github.com/teispace/teitunnel/commit/12ffe01cc25c84a11d959a1e45dad4f5e25a8cf3))
* **cf-api:** Access applications, organization and login methods ([1ac6216](https://github.com/teispace/teitunnel/commit/1ac6216eb78a024d26b85c0a37bfb8a966610bc9))
* **cf-api:** client foundation with retries, rate limit, pagination and zones ([cbbb7b5](https://github.com/teispace/teitunnel/commit/cbbb7b55d3e2127fb865e69a1ee048703914ff21))
* **cf-api:** tunnels, remote configuration and DNS records ([31db541](https://github.com/teispace/teitunnel/commit/31db541b9b99e029754b7c7052526a3cba041df3))
* **cli:** install the bundled command line tool from Settings ([19d862a](https://github.com/teispace/teitunnel/commit/19d862ad361f7837da8bdde0343e9aef04175367))
* **cli:** list and stop shares on your domains ([6cfa74d](https://github.com/teispace/teitunnel/commit/6cfa74d59467977499603e54f519a44b49c30cb0))
* **cli:** run on servers and in containers ([d65217c](https://github.com/teispace/teitunnel/commit/d65217c937555de70337bd061a4afb358a640b5e))
* **cli:** serve a web dashboard and API on servers ([046834c](https://github.com/teispace/teitunnel/commit/046834c9855945c8020c504fc36fd359c31f1209))
* **cli:** share, doctor and shell completions ([b155c64](https://github.com/teispace/teitunnel/commit/b155c64c019f072e98bb3eaad886f60939d86f7d))
* **cli:** teitunnel-cli for routes and exports from the terminal ([618abec](https://github.com/teispace/teitunnel/commit/618abecc311fdfa43a0c3d433676b64db4e2a498))
* **cloudflared:** launchd agent plist and typed launchctl invocations ([c05c219](https://github.com/teispace/teitunnel/commit/c05c219c12c35a9d4ea49d8a5ce06e50ba5ef7c4))
* **cloudflared:** local endpoints client and Prometheus metrics parser ([baef251](https://github.com/teispace/teitunnel/commit/baef251b3ac3bc4c146521a14961f4b3a559bd16))
* **cloudflared:** locate binaries and build commands without secrets in argv ([ef83ebd](https://github.com/teispace/teitunnel/commit/ef83ebdd1115e188d579d0bc1c633899cdbd34b8))
* **cloudflared:** parse and classify JSON log lines ([2349a0c](https://github.com/teispace/teitunnel/commit/2349a0c36a2545344f27de451e918765888a8485))
* **cloudflared:** verified managed install with progress ([231fe97](https://github.com/teispace/teitunnel/commit/231fe97455abd57d268593b7946c5f0754511a48))
* **connectors:** move running connectors onto an updated cloudflared without a gap ([d9a5b6b](https://github.com/teispace/teitunnel/commit/d9a5b6b9cb8ed02a0cfd0192afb94307ef5a2562))
* **discovery:** cloudflared processes Teitunnel didn't start ([7349b0b](https://github.com/teispace/teitunnel/commit/7349b0bd8fef2093440bac6227d74c4ddd22d1ea))
* **discovery:** Docker containers' published ports in the service list ([3f05031](https://github.com/teispace/teitunnel/commit/3f050319bdda49885459d6fb2959f45e652c4e17))
* **discovery:** framework hints and project names from more manifests ([4ed3f4d](https://github.com/teispace/teitunnel/commit/4ed3f4d81f57084e06cd7e790fb79a145bf0c960))
* **discovery:** list local services with process, kind and project ([60086a8](https://github.com/teispace/teitunnel/commit/60086a84fbfca67065a14e78ad268445b4c26b01))
* **doctor:** add cloudflared's own report to a diagnostics export ([a836955](https://github.com/teispace/teitunnel/commit/a836955b6426a30ec9899c71ecf6f488b3deca89))
* **doctor:** checks for binary, permissions, domains, DNS, connector, origins and orphans ([aec2e40](https://github.com/teispace/teitunnel/commit/aec2e40fa9ea6704f0b0bc647b1a494bdba34c9b))
* **doctor:** Doctor view with sidebar badge, fixes as reviewed plans, ignore ([47ece87](https://github.com/teispace/teitunnel/commit/47ece871a3f08ed87b8e026222b376bbfe454b8f))
* **doctor:** export redacted diagnostics ([6a14452](https://github.com/teispace/teitunnel/commit/6a1445271ed1750c0ac1ba763490649756b93078))
* **doctor:** find and remove logins left without their route ([d544232](https://github.com/teispace/teitunnel/commit/d544232a47da02fb6f3c551f13b880c4f2fbcde3))
* **doctor:** fix all safe issues ([15f82bf](https://github.com/teispace/teitunnel/commit/15f82bf89693d05e2b54a94fcf9c6d33d22f76d6))
* **doctor:** notify about new problems, also with the window closed ([26a281a](https://github.com/teispace/teitunnel/commit/26a281a00dd5d0d77922b04cd21c75b91f392414))
* **doctor:** show issues on the routes and tunnels they affect ([fa4b2d5](https://github.com/teispace/teitunnel/commit/fa4b2d5a5f1d5847f4ea526eb6da30669ab823ac))
* **doctor:** stale connections and duplicate local connector checks ([1c94d9a](https://github.com/teispace/teitunnel/commit/1c94d9a3164b0f82c8c1fcb052a33dd3362603a3))
* **domain:** hostname, route origin and path rule types ([4e228a8](https://github.com/teispace/teitunnel/commit/4e228a8c9892d7f184f537e3dddded3a530edf3e))
* **engine:** drift detection with keep-theirs and restore-mine ([9172905](https://github.com/teispace/teitunnel/commit/917290520d9020ee60e04008ad05822c60fbb249))
* **engine:** observer, executor with rollback, ownership index and activity log ([f4d0ccf](https://github.com/teispace/teitunnel/commit/f4d0ccfd6299358484967f31dfc379bf3c8dc10c))
* **engine:** pure planner for routes across zones ([96346a9](https://github.com/teispace/teitunnel/commit/96346a9485f450f7515a0d052908df237987dbe1))
* **engine:** staged route verifier that never resolves the new hostname ([181f48a](https://github.com/teispace/teitunnel/commit/181f48a80c19ad0ecae6a24ecc6c493b3198f366))
* **export:** config.yml, Docker Compose and Terraform exports of this Mac's routes ([696eeeb](https://github.com/teispace/teitunnel/commit/696eeeb1755ea92f9a24dbcd30b6a3d262ef8eaf))
* fix permission and setup gaps in place everywhere ([d693cac](https://github.com/teispace/teitunnel/commit/d693cac08cbd85d5c3b33bec7cbf5f55fe4c233e))
* **i18n:** translate activity, doctor, overview, domains, settings and accounts ([0fba83b](https://github.com/teispace/teitunnel/commit/0fba83bfe70e0af077991c9dc7125e5e96ffa754))
* **i18n:** translate quick share, the app shell and shared components ([875bbde](https://github.com/teispace/teitunnel/commit/875bbdeaa2e5723acebb70e06bf1852e0d7b28a6))
* **i18n:** translate text produced in Rust ([983b329](https://github.com/teispace/teitunnel/commit/983b329850b6f9d4fd10255e42825d0dd57d90f1))
* **i18n:** typed message catalogs; translate the routes and tunnels views ([400d9cc](https://github.com/teispace/teitunnel/commit/400d9ccb46e363572f67d4b361b3849ac6820df1))
* **import:** bring routes from existing cloudflared configs onto this Mac's tunnel ([6ac5a4e](https://github.com/teispace/teitunnel/commit/6ac5a4e46cc0320dd1c655e6f5edbab02b461f2a))
* initial commit for Teitunnel desktop control center ([5e96c72](https://github.com/teispace/teitunnel/commit/5e96c7298324b1175dfc641d4fab96e86f301f8c))
* **logs:** log viewer with follow-tail, level filter, search, pause and copy ([841085c](https://github.com/teispace/teitunnel/commit/841085c34be711571c748d950ac90e89b83f3eec))
* **logs:** per-route logs, and bounded Always-on log files ([9aef10a](https://github.com/teispace/teitunnel/commit/9aef10ab7bd7d62a3f4b06ef334be0d412eeb155))
* **logs:** virtualized log viewer and save to a file ([1dba178](https://github.com/teispace/teitunnel/commit/1dba178797c79bbff8a6a19730b56aa23cd20210))
* menu bar shares, notifications and an Overview of what's running ([822224b](https://github.com/teispace/teitunnel/commit/822224b46a3d4e0a0ad28eaf483a1c0fbeb63ca1))
* **menu:** a complete Help menu and About credits ([2fe255e](https://github.com/teispace/teitunnel/commit/2fe255e53271048f35b20f460cb4f352abfc0dba))
* **networks:** share private networks with WARP clients and connect to SSH/TCP routes ([61aa537](https://github.com/teispace/teitunnel/commit/61aa5377042596f05d4a01948f7a6c0615adddee))
* **notifications:** tell the user when this Mac's connector goes down or comes back ([be45961](https://github.com/teispace/teitunnel/commit/be459612ddec561ec33208638fd84818a58ef008))
* **oauth:** PKCE loopback sign-in protocol ([07ab573](https://github.com/teispace/teitunnel/commit/07ab573f09f1dfbff55333ffaa4eedd235f6f108))
* **oauth:** sign in with Cloudflare, token refresh and revocation ([1d00f52](https://github.com/teispace/teitunnel/commit/1d00f52a1298b85dbc4b9544a94517f388fd77e8))
* **overview:** routes and problems at a glance ([8663f90](https://github.com/teispace/teitunnel/commit/8663f9084d8850c4fe8711814a1cb18be589e732))
* **overview:** this Mac's traffic at a glance ([aaa50ba](https://github.com/teispace/teitunnel/commit/aaa50ba15fc187af289ffbc8381fd1c42377d092))
* **platform:** Windows and Linux groundwork for Always-on, consoles and credentials ([94cab95](https://github.com/teispace/teitunnel/commit/94cab95f31adb79bb5895a763a35fad34bfb09e2))
* **quick-share:** core service with live URL detection, stats and history ([09a1f89](https://github.com/teispace/teitunnel/commit/09a1f897760a321aabbb521a158f4e36b342e318))
* **quick-share:** desktop commands, exit hook and the Quick Share screen ([3efc797](https://github.com/teispace/teitunnel/commit/3efc797e6583c14fe884a40b2876062309316fde))
* **quick-share:** list and stop shares started in a terminal ([048b51d](https://github.com/teispace/teitunnel/commit/048b51df8d4946fc36226b77c50d867ba94a6e98))
* **quick-share:** share on your own domain ([bcc8dc3](https://github.com/teispace/teitunnel/commit/bcc8dc3f91605d364ef086383056be04d57fa91c))
* **quick-share:** wait out DNS propagation before going live; nightly real test ([5bd8163](https://github.com/teispace/teitunnel/commit/5bd816313958b765e2dba4a42e430f5957d0dfbe))
* **routes:** Doctor issues show inline on route rows and in the inspector status ([b8c1f71](https://github.com/teispace/teitunnel/commit/b8c1f716f2a2db5b4afec32ca22e877bf42f22c9))
* **routes:** every origin setting in the app, the CLI and imports ([f44852b](https://github.com/teispace/teitunnel/commit/f44852bf1d917d7d0d2a1588ab865d4d611a6db5))
* **routes:** IPC for overview, preview, apply with progress, verify, drift and activity ([29a1813](https://github.com/teispace/teitunnel/commit/29a18132a06f86e5527322fcea604c1a97027d7d))
* **routes:** load balance a route across machines ([f682560](https://github.com/teispace/teitunnel/commit/f68256002faf3088d7d1fc8344e02797245e6aad))
* **routes:** require a login for a route with Cloudflare Access ([07f613e](https://github.com/teispace/teitunnel/commit/07f613ea8f0ed3bc4d24857312260983ef87fafb))
* **routes:** Routes view, route sheet with plan review, live apply progress and verification ([294b98d](https://github.com/teispace/teitunnel/commit/294b98dd512cfd4deb85383f1be4e588337c24fb))
* **routes:** show each machine's health behind a load-balanced route ([bc41e55](https://github.com/teispace/teitunnel/commit/bc41e556798ecccc980c0b22cef2d428b846cfec))
* **runtime:** connector supervisor with health, backoff and orphan safety ([5b17253](https://github.com/teispace/teitunnel/commit/5b17253ac6b0db794df98c8fdbf6f72ed7124063))
* **service:** one neutral service definition with launchd, systemd and Task Scheduler renderers ([2e3eb5e](https://github.com/teispace/teitunnel/commit/2e3eb5e8bb86a453e328125dd2392bc872bfa078))
* **settings:** notification switches for connectors and Quick Shares ([6fff9ca](https://github.com/teispace/teitunnel/commit/6fff9cad4f5c92997948e56ec46cb2d1041fc905))
* **shell:** follow the system accent colour via AppKit ([321dfee](https://github.com/teispace/teitunnel/commit/321dfeea582ce14a7fe0535af52c4e5a523de7c3))
* **shell:** native menu bar, menu bar extra and command palette ([e7cf9fa](https://github.com/teispace/teitunnel/commit/e7cf9fa0f51233e4c981026f044af9d284d02dd3))
* **shell:** native Windows and Linux chrome and wording ([bc3e16f](https://github.com/teispace/teitunnel/commit/bc3e16f7036c33c0fdbf9069bbeadfb2c217b2b0))
* **store:** SQLite store, typed settings and the Settings window ([aa0f6ac](https://github.com/teispace/teitunnel/commit/aa0f6ac77643a4c10fe3b7572ec0e9c3d0ffb728))
* **traffic:** live 1 s metrics, 7-day history and uPlot charts ([6b96e8a](https://github.com/teispace/teitunnel/commit/6b96e8a7b0e8fba5c18eb3aea5a5013b528d950d))
* **tray:** health line above the routes in the menu bar menu ([a5cfd1c](https://github.com/teispace/teitunnel/commit/a5cfd1caa4b7f9c3e6412332fff6faf6d53c2683))
* **tray:** routes with status in the menu bar menu ([55a9b37](https://github.com/teispace/teitunnel/commit/55a9b379ae1a53e1d293c06e5e737970fb86d431))
* **tray:** start and stop this Mac's routes from the menu bar ([2ecff66](https://github.com/teispace/teitunnel/commit/2ecff666665f95fd9a4f6e82e2aa65bf41de4bf6))
* **tray:** the menu bar icon shows a dot when a route isn't working ([99ea94c](https://github.com/teispace/teitunnel/commit/99ea94c30aec285f83bc4d7792944513b02cdc2f))
* **tunnels:** connector logs and log-based Doctor checks ([c37fbaf](https://github.com/teispace/teitunnel/commit/c37fbaf6de0d312f8f1ef94946544c36bd7124b0))
* **tunnels:** machines per tunnel and live logs of remote connectors ([8ad0410](https://github.com/teispace/teitunnel/commit/8ad0410f17104a1738f004c1f5ade57771adeee8))
* **tunnels:** run an existing tunnel on this machine ([76de74e](https://github.com/teispace/teitunnel/commit/76de74e77b67c35ffd27b7030fe8b593ffe33d0b))
* **tunnels:** several tunnels per machine ([0faf9e4](https://github.com/teispace/teitunnel/commit/0faf9e477d274e1e344b22ec655547684cbde9a4))
* **tunnels:** this Mac's connector with keychain token and stable metrics port ([2a1d77f](https://github.com/teispace/teitunnel/commit/2a1d77fbb173afeb4e25c0044c7e0c6241757d3e))
* **tunnels:** traffic of this Mac's connector ([fe3767b](https://github.com/teispace/teitunnel/commit/fe3767b3fa611ac5f877e1896a78ca7698b44258))
* **tunnels:** Tunnels view with this Mac's connector controls ([ecc8f16](https://github.com/teispace/teitunnel/commit/ecc8f167152c105f02daea7756361df34ee75f6a))
* **ui:** native primitives, grouped list, copy field and dev gallery ([9ac6448](https://github.com/teispace/teitunnel/commit/9ac64482972dcb62af328f1f7507792cae596069))
* **ui:** split view, list pane, inspector and resizable sidebar ([4f17adf](https://github.com/teispace/teitunnel/commit/4f17adfe2e0274a1368e9432190bd38eebe39169))
* **updates:** check, download and install app updates ([e7a35b7](https://github.com/teispace/teitunnel/commit/e7a35b7c55c25a517bb6ed03766dba8db34fa947))
* **web:** animated landing page, rebuilt download page, SEO and deeper docs ([#16](https://github.com/teispace/teitunnel/issues/16)) ([3474020](https://github.com/teispace/teitunnel/commit/34740204d9118d587ff6c39b19023c8b62bf28aa))
* **web:** download pages that pick the right file for each system ([91c70df](https://github.com/teispace/teitunnel/commit/91c70dfa0b1dff8f745a4329d899ed0d2685537a))
* **web:** landing page and Fumadocs docs site ([f6ffa99](https://github.com/teispace/teitunnel/commit/f6ffa99e83be6c4c57a50fe59f73151bd1c56790))
* workspace, core crates, Tauri shell and typed IPC foundation ([47352a2](https://github.com/teispace/teitunnel/commit/47352a2e125dcd4f6e48793548a09424d38d7494))


### Bug Fixes

* **cf-api:** a DELETE answering success with a null result is a success. ([8ac9d07](https://github.com/teispace/teitunnel/commit/8ac9d077403558c0221b52760bb9c7ce79af4850))
* **core:** clear an abandoned registry even if its owner died mid-write ([#18](https://github.com/teispace/teitunnel/issues/18)) ([2190f1a](https://github.com/teispace/teitunnel/commit/2190f1a47af8205faacf9b6ccfe8ef2e74f90c8f))
* **core:** scope the service tests to Unix so Windows clippy passes ([559ccd8](https://github.com/teispace/teitunnel/commit/559ccd8ad4140402ae1e5e49ecaa2fdb618ed27e))
* **deps:** upgrade postcss past its security advisories ([#12](https://github.com/teispace/teitunnel/issues/12)) ([453f7e8](https://github.com/teispace/teitunnel/commit/453f7e83cc51afc94e9584bbcc240b21e7c5d1bc))
* **desktop:** repair an invalid navigator.language before views load ([3069e82](https://github.com/teispace/teitunnel/commit/3069e82cf0a6382d272f441f70947f2e73c86305))
* **e2e:** build the E2E app into target/e2e, never over the normal debug app ([dc2d2de](https://github.com/teispace/teitunnel/commit/dc2d2dec5d42e1e4816395adf2810153fd79361b))
* **origin:** require brackets around IPv6 hosts ([952f101](https://github.com/teispace/teitunnel/commit/952f10107ed2fef6ff962128b305fa36509051f1))
* **quick-share:** keep the service list open when the field is clicked ([9b46d31](https://github.com/teispace/teitunnel/commit/9b46d312d42119b0ab59c377d6670c90f0de2494))
* **release:** give universal Mac builds each architecture's CLI ([0f4c92f](https://github.com/teispace/teitunnel/commit/0f4c92f5993d70001f72344c745ff2c4ef14e787))
* **runtime:** only import process-group signalling on Unix ([c0982e4](https://github.com/teispace/teitunnel/commit/c0982e420f735e6aaf3356538471dac829e8831e))
* **runtime:** write process records atomically ([783e9fd](https://github.com/teispace/teitunnel/commit/783e9fd0103221c0f7b984af6c660bd0328c6be4))
* **webview:** don't log ResizeObserver loop notices as uncaught errors ([94cb500](https://github.com/teispace/teitunnel/commit/94cb500ad2490bcb2328f383004290225072697b))


### Performance

* **app:** measure cold start and idle memory of a packaged build ([1929653](https://github.com/teispace/teitunnel/commit/1929653d3bfa0936bbbaee209503e65f999435ad))
