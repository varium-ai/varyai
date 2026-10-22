import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { describe, expect, it } from "vitest";

function check(source) {
  const result = spawnSync(
    process.execPath,
    ["--input-type=module", "-e", source],
    { encoding: "utf8" },
  );
  expect(result.status, result.stderr).toBe(0);
}

describe("Windows cross compilation", () => {
  it("loads and builds with simulated tools without private release facilities", () => {
    const directory = mkdtempSync(path.join(tmpdir(), "varyai-public-cross-"));
    try {
      const scripts = path.join(directory, "scripts");
      mkdirSync(path.join(scripts, "lib"), { recursive: true });
      copyFileSync(
        "scripts/build-windows-cross.mjs",
        path.join(scripts, "build-windows-cross.mjs"),
      );
      if (existsSync("scripts/lib/command.mjs")) {
        copyFileSync(
          "scripts/lib/command.mjs",
          path.join(scripts, "lib/command.mjs"),
        );
      }
      check(`
        import assert from 'node:assert/strict';
        import { buildWindowsCross } from ${JSON.stringify(pathToFileURL(path.join(scripts, "build-windows-cross.mjs")).href)};
        const calls=[];
        buildWindowsCross({os:'darwin',env:{PATH:'/bin'},run:(program,args)=>{
          calls.push([program,args]);
          return {status:0,stdout:program==='brew'?'/toolchain/'+args[1]:args[0]==='--coff-imports'?'Name: KERNEL32.dll':''};
        }});
        assert.ok(calls.some(([program])=>program==='pnpm'));
        assert.equal(calls.at(-1)[0],'llvm-readobj');
      `);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  it("builds NSIS with cargo-xwin and keeps Apple signing out of the Windows build", () => {
    check(`
      import assert from 'node:assert/strict';
      import { buildWindowsCross } from './scripts/build-windows-cross.mjs';
      const calls = [];
      const run = (program, args, options) => {
        calls.push([program, args, options]);
        return {status:0,stdout:program === 'brew' ? '/opt/homebrew/opt/' + args[1] + '\\n' : args[0] === '--coff-imports' ? 'Name: KERNEL32.dll\\nName: ucrtbase.dll' : ''};
      };
      buildWindowsCross({os:'darwin',run,env:{PATH:'/bin',HOME:'/Users/example',LANG:'C.UTF-8',LC_ALL:'C.UTF-8',STATIC_VCRUNTIME:'false',APPLE_SIGNING_IDENTITY:'apple',APPLE_ID:'private',TAURI_SIGNING_PRIVATE_KEY:'test-key'}});
      const [program, args, options] = calls.findLast(([program])=>program==='pnpm');
      assert.equal(program,'pnpm');
      assert.deepEqual(args.slice(0,6),['tauri','build','--runner','cargo-xwin','--target','x86_64-pc-windows-msvc']);
      assert.ok(!args.includes('--bundles'));
      assert.deepEqual(JSON.parse(args[args.indexOf('--config')+1]),{bundle:{targets:['nsis'],createUpdaterArtifacts:false}});
      assert.ok(options.env.PATH.includes('/opt/homebrew/opt/llvm/bin'));
      assert.ok(options.env.PATH.includes('/opt/homebrew/opt/lld/bin'));
      assert.equal(options.env.XWIN_CACHE_DIR,'/Users/example/.cache/varyai/xwin');
      assert.equal(options.env.XWIN_ARCH,'x86_64');
      assert.equal(options.env.LANG,'en_US.UTF-8');
      assert.equal(options.env.LC_ALL,'en_US.UTF-8');
      assert.equal(options.env.LC_CTYPE,'en_US.UTF-8');
      assert.equal(options.env.STATIC_VCRUNTIME,'true');
      assert.ok(!('APPLE_SIGNING_IDENTITY' in options.env));
      assert.ok(!('APPLE_ID' in options.env));
      assert.equal(options.env.TAURI_SIGNING_PRIVATE_KEY,'test-key');
      assert.ok(calls.some(([p,a])=>p==='llvm-rc'&&JSON.stringify(a)===JSON.stringify(['/?'])));
      buildWindowsCross({os:'darwin',run,updater:true,env:{HOME:'/Users/example'}});
      const updaterArgs=calls.findLast(([program])=>program==='pnpm')[1];
      assert.deepEqual(JSON.parse(updaterArgs[updaterArgs.indexOf('--config')+1]),{bundle:{targets:['nsis'],createUpdaterArtifacts:true}});
      assert.throws(() => buildWindowsCross({os:'linux',run}),/macOS/);
    `);
  });

  it("rejects external Visual C++ runtime dependencies before reporting build success", () => {
    check(`
      import assert from 'node:assert/strict';
      import { verifyWindowsRuntime } from './scripts/build-windows-cross.mjs';
      const runFor=(stdout,status=0)=>(program,args,options)=>{
        assert.equal(program,'llvm-readobj');
        assert.deepEqual(args,['--coff-imports','src-tauri/target/x86_64-pc-windows-msvc/release/varyai.exe']);
        assert.equal(options.capture,true);
        return {status,stdout};
      };
      verifyWindowsRuntime({run:runFor('Name: KERNEL32.dll\\nName: api-ms-win-crt-runtime-l1-1-0.dll')});
      for(const dll of ['VCRUNTIME140.dll','vcruntime140_1.dll','MSVCP140.dll']) {
        assert.throws(()=>verifyWindowsRuntime({run:runFor('Name: KERNEL32.dll\\nName: '+dll)}),/Visual C\\+\\+ runtime/);
      }
      assert.throws(()=>verifyWindowsRuntime({run:runFor('invalid output')}),/inspect/);
      assert.throws(()=>verifyWindowsRuntime({run:runFor('Name: KERNEL32.dll',1)}),/inspect/);
    `);
  });

  it("installs only missing cross-compilation tools", () => {
    check(`
      import assert from 'node:assert/strict';
      import { setupWindowsCross } from './scripts/build-windows-cross.mjs';
      const calls=[];
      const run=(program,args,options)=>{
        calls.push([program,args,options]);
        return {status:args[0]==='list'||(program==='cargo-xwin'&&args[0]==='--version')?1:0,stdout:''};
      };
      setupWindowsCross({os:'darwin',run,env:{}});
      assert.ok(calls.some(([p,a])=>p==='brew'&&JSON.stringify(a)===JSON.stringify(['install','llvm','lld','nsis','cmake','ninja'])));
      assert.ok(calls.some(([p,a])=>p==='cargo'&&JSON.stringify(a)===JSON.stringify(['install','--locked','cargo-xwin','--version','0.23.1'])));
      const ready=[];
      setupWindowsCross({os:'darwin',run:(...args)=>{ready.push(args);return {status:0,stdout:''};},env:{}});
      assert.ok(!ready.some(([,args])=>args[0]==='install'));
    `);
  });

  it("explains how to install a missing cross compiler before attempting a build", () => {
    check(`
      import assert from 'node:assert/strict';
      import { buildWindowsCross } from './scripts/build-windows-cross.mjs';
      const calls=[];
      const run=(program,args,options)=>{
        calls.push([program,args,options]);
        if (program==='cargo-xwin') return {status:null,error:{code:'ENOENT'}};
        return {status:0,stdout:program==='brew'?'/opt/homebrew/opt/'+args[1]+'\\n':''};
      };
      assert.throws(()=>buildWindowsCross({os:'darwin',run,env:{PATH:'/bin'}}),/cargo-xwin.*pnpm build:windows setup/);
      assert.ok(!calls.some(([program])=>program==='pnpm'));
    `);
  });

  it.skipIf(process.platform !== "darwin")(
    "passes the real macOS Tauri CLI parser and reaches the Windows runner",
    () => {
      const directory = mkdtempSync(path.join(tmpdir(), "varyai-cross-cli-"));
      try {
        const bin = path.join(directory, "bin");
        const backend = path.join(directory, "src-tauri");
        mkdirSync(bin);
        mkdirSync(path.join(backend, "src"), { recursive: true });
        mkdirSync(path.join(directory, "dist"));
        writeFileSync(path.join(directory, "dist/index.html"), "<html></html>");
        writeFileSync(
          path.join(backend, "Cargo.toml"),
          '[package]\nname = "varyai-parser-fixture"\nversion = "0.0.0"\nedition = "2021"\n',
        );
        writeFileSync(path.join(backend, "src/main.rs"), "fn main() {}\n");
        writeFileSync(
          path.join(backend, "tauri.conf.json"),
          JSON.stringify({
            productName: "Parser Fixture",
            version: "0.0.0",
            identifier: "example.varyai.parser",
            build: { frontendDist: "../dist" },
          }),
        );
        // Use a fixture target inventory; the runner exits before compilation or SDK downloads.
        writeFileSync(
          path.join(bin, "rustup"),
          '#!/bin/sh\n[ "$1 $2" = "target list" ] || exit 74\nprintf "x86_64-pc-windows-msvc (installed)\\n"\n',
          { mode: 0o755 },
        );
        writeFileSync(
          path.join(bin, "cargo-xwin"),
          '#!/bin/sh\n[ "$1" = build ] || exit 74\ncase "$*" in *--target*x86_64-pc-windows-msvc*) ;; *) exit 75 ;; esac\nprintf "VARYAI_WINDOWS_RUNNER_REACHED\\n" >&2\nexit 73\n',
          { mode: 0o755 },
        );
        const cargoBin = path.join(
          process.env.CARGO_HOME || path.join(homedir(), ".cargo"),
          "bin",
        );
        check(`
        import assert from 'node:assert/strict';
        import { spawnSync } from 'node:child_process';
        import { buildWindowsCross } from './scripts/build-windows-cross.mjs';
        let args;
        buildWindowsCross({os:'darwin',run:(program,values)=>{
          if(program==='pnpm')args=values;
          return {status:0,stdout:program==='brew'?'/opt/homebrew/opt/'+values[1]+'\\n':values[0]==='--coff-imports'?'Name: KERNEL32.dll':''};
        }});
        const result=spawnSync(process.execPath,[${JSON.stringify(path.resolve("node_modules/@tauri-apps/cli/tauri.js"))},...args.slice(1)],{
          cwd:${JSON.stringify(directory)},encoding:'utf8',timeout:30000,
          env:{...process.env,CARGO_NET_OFFLINE:'true',PATH:${JSON.stringify([bin, cargoBin].join(path.delimiter))}+${JSON.stringify(path.delimiter)}+process.env.PATH},
        });
        assert.ok(!result.error,result.error?.message);
        assert.notEqual(result.status,0,'The stub must prevent compilation and SDK downloads');
        assert.ok((result.stdout+result.stderr).includes('VARYAI_WINDOWS_RUNNER_REACHED'),result.stdout+result.stderr);
      `);
      } finally {
        rmSync(directory, { recursive: true, force: true });
      }
    },
  );
});
