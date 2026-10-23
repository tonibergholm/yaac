import { render } from 'preact';
import { HttpBridge } from './bridge/http-bridge';
import type { Bridge } from './bridge/types';
import { Sidebar } from './state/sidebar';
import { App } from './ui/App';
import { guardForeignDrops } from './ui/guards';
import './styles.css';

/**
 * The host serves this page from app://. Anywhere else, such as the Vite dev server, the in-memory mock
 * core is used, so `pnpm dev` works in a plain browser. The mock is a dev-only chunk; it is
 * not part of the production build.
 */
async function chooseBridge(): Promise<Bridge> {
  if (import.meta.env.DEV && window.location.protocol !== 'app:') {
    const { MockBridge } = await import('./bridge/mock-bridge');
    return new MockBridge();
  }
  return new HttpBridge();
}

const root = document.getElementById('app');
if (!root) throw new Error('sidebar: missing #app element');

guardForeignDrops();
const sidebar = new Sidebar(await chooseBridge());
render(<App sidebar={sidebar} />, root);
sidebar.start();
