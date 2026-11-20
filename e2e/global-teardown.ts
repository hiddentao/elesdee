import { removeRunDir } from './env';

export default async function globalTeardown(): Promise<void> {
  removeRunDir();
}
