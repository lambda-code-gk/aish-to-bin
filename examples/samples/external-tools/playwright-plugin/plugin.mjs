#!/usr/bin/env node
/**
 * AISH 外部ツールプラグイン（Playwright サンプル）
 *
 * stdio で JSON-RPC 2.0 を話し、ブラウザ操作ツールを提供する。
 * 起動: node plugin.mjs （または manifest から command/args で指定）
 *
 * 必要: npm install playwright
 */

import * as readline from 'readline';
import { mkdtempSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';
import { chromium } from 'playwright';

let browser = null;
let page = null;

function jsonRpcSuccess(id, result) {
  return { jsonrpc: '2.0', id, result };
}

function jsonRpcError(id, code, message) {
  return { jsonrpc: '2.0', id, error: { code, message } };
}

const TOOLS = [
  {
    name: 'browser_launch',
    description: 'Launch a Chromium browser. Call this first before other browser_* tools. Set headless to false to show the window.',
    input_schema: {
      type: 'object',
      properties: {
        headless: { type: 'boolean', description: 'Run browser without GUI', default: true },
      },
    },
  },
  {
    name: 'browser_goto',
    description: 'Navigate the current page to a URL. Requires browser_launch first.',
    input_schema: {
      type: 'object',
      properties: {
        url: { type: 'string', description: 'URL to open (e.g. https://example.com)' },
      },
      required: ['url'],
    },
  },
  {
    name: 'browser_content',
    description: 'Get the main text content of the current page (body innerText). Use after browser_goto.',
    input_schema: {
      type: 'object',
      properties: {
        maxChars: { type: 'number', description: 'Max characters to return (default 10000)', default: 10000 },
      },
    },
  },
  {
    name: 'browser_click',
    description: 'Click an element selected by CSS selector (e.g. "button.submit", "a#link").',
    input_schema: {
      type: 'object',
      properties: {
        selector: { type: 'string', description: 'CSS selector of the element to click' },
      },
      required: ['selector'],
    },
  },
  {
    name: 'browser_fill',
    description: 'Fill an input field selected by CSS selector. Use for text inputs and search boxes.',
    input_schema: {
      type: 'object',
      properties: {
        selector: { type: 'string', description: 'CSS selector of the input' },
        value: { type: 'string', description: 'Text to type' },
      },
      required: ['selector', 'value'],
    },
  },
  {
    name: 'browser_screenshot',
    description: 'Take a screenshot of the current page. Returns the path to the saved PNG file.',
    input_schema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'Optional file path to save (default: temp file)' },
      },
    },
  },
  {
    name: 'browser_close',
    description: 'Close the browser and release resources. Call when done with browser tools.',
    input_schema: { type: 'object', properties: {} },
  },
];

async function handleInitialize(params) {
  return {};
}

async function handleListTools() {
  return TOOLS;
}

async function handleCallTool(name, args) {
  const a = args || {};

  switch (name) {
    case 'browser_launch': {
      if (browser) await browser.close();
      browser = await chromium.launch({ headless: a.headless !== false });
      const ctx = browser.contexts()[0] || await browser.newContext();
      page = ctx.pages()[0] || await ctx.newPage();
      return { ok: true, message: 'Browser launched' };
    }
    case 'browser_goto': {
      if (!page) return { error: 'Call browser_launch first' };
      const url = a.url || 'about:blank';
      const res = await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30000 });
      return {
        ok: true,
        url: page.url(),
        status: res ? res.status() : null,
        title: await page.title(),
      };
    }
    case 'browser_content': {
      if (!page) return { error: 'Call browser_launch first' };
      const maxChars = a.maxChars || 10000;
      const text = await page.evaluate(() => document.body ? document.body.innerText : '');
      const truncated = text.length > maxChars ? text.slice(0, maxChars) + '...[truncated]' : text;
      return { ok: true, content: truncated, totalLength: text.length };
    }
    case 'browser_click': {
      if (!page) return { error: 'Call browser_launch first' };
      const selector = a.selector;
      if (!selector) return { error: 'selector is required' };
      await page.click(selector, { timeout: 10000 });
      return { ok: true, message: `Clicked ${selector}` };
    }
    case 'browser_fill': {
      if (!page) return { error: 'Call browser_launch first' };
      const selector = a.selector;
      const value = a.value;
      if (!selector || value === undefined) return { error: 'selector and value are required' };
      await page.fill(selector, String(value), { timeout: 10000 });
      return { ok: true, message: `Filled ${selector}` };
    }
    case 'browser_screenshot': {
      if (!page) return { error: 'Call browser_launch first' };
      const out = a.path || join(mkdtempSync(join(tmpdir(), 'aish-')), 'screenshot.png');
      await page.screenshot({ path: out });
      return { ok: true, path: out };
    }
    case 'browser_close': {
      if (browser) {
        await browser.close();
        browser = null;
        page = null;
      }
      return { ok: true, message: 'Browser closed' };
    }
    default:
      return { error: `Unknown tool: ${name}` };
  }
}

async function handleRequest(req) {
  const id = req.id;
  const method = req.method;
  const params = req.params || {};

  try {
    if (method === 'initialize') {
      return jsonRpcSuccess(id, await handleInitialize(params));
    }
    if (method === 'list_tools') {
      return jsonRpcSuccess(id, await handleListTools());
    }
    if (method === 'call_tool') {
      const name = params.name;
      const arguments_ = params.arguments;
      const result = await handleCallTool(name, arguments_);
      return jsonRpcSuccess(id, { content: result });
    }
    return jsonRpcError(id, -32601, `Method not found: ${method}`);
  } catch (err) {
    return jsonRpcSuccess(id, {
      content: { error: err.message || String(err) },
    });
  }
}

async function main() {
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
  for await (const line of rl) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const req = JSON.parse(trimmed);
      const res = await handleRequest(req);
      console.log(JSON.stringify(res));
    } catch (e) {
      console.log(
        JSON.stringify({
          jsonrpc: '2.0',
          id: null,
          error: { code: -32700, message: 'Parse error: ' + (e.message || String(e)) },
        })
      );
    }
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
