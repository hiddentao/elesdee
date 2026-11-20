import type { AiConfig, Provider } from '../api/types';
import type { Language } from '../i18n';
import { providerUsage } from '../state/usage';
import type { ComboboxOption } from './Combobox';

export function configOption(
  config: AiConfig,
  providers: Provider[],
  language: Language,
): ComboboxOption {
  const provider =
    providers.find((item) => item.id === config.providerId) ?? null;
  const details = [
    config.client,
    provider?.name,
    config.model,
    provider ? providerUsage(provider, language).summary : '',
  ].filter(Boolean);
  const usage = provider ? providerUsage(provider, language).summary : '';
  return {
    id: config.id,
    label: config.name,
    closedLabel: usage ? `${config.name} · ${usage}` : config.name,
    meta: details.join(' · '),
  };
}
