import './styles/shell.css';
import { ShellApp } from './app';

const root = document.getElementById('app');

if (!root) {
  throw new Error('The application root is missing.');
}

void new ShellApp(root).start();
