import { useState } from 'react';
import type { Question } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';

interface QuestionFormProps {
  question: Question;
  onAnswer: (answer: string) => Promise<void>;
  compact?: boolean;
}

export function QuestionForm({
  question,
  onAnswer,
  compact = false,
}: QuestionFormProps) {
  const { t } = useLanguage();
  const [answer, setAnswer] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(value: string) {
    const trimmed = value.trim();
    if (!trimmed) {
      setError(t('question.errorEmpty'));
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await onAnswer(trimmed);
    } catch {
      setSubmitting(false);
      setError(t('question.errorSend'));
    }
  }

  return (
    <div className={compact ? 'question-form is-compact' : 'question-form'}>
      <p className="question-text">{question.text}</p>
      {question.options.length > 0 ? (
        <div className="question-options">
          {question.options.map((option) => (
            <button
              key={option}
              type="button"
              className="btn btn-ghost"
              disabled={submitting}
              onClick={() => void submit(option)}
            >
              {option}
            </button>
          ))}
        </div>
      ) : (
        <form
          className="question-answer"
          onSubmit={(event) => {
            event.preventDefault();
            void submit(answer);
          }}
        >
          <input
            className="text-input"
            value={answer}
            aria-label={t('question.answerLabel')}
            placeholder={t('question.answerPlaceholder')}
            disabled={submitting}
            onChange={(event) => {
              setAnswer(event.target.value);
              setError(null);
            }}
          />
          <button
            type="submit"
            className="btn btn-primary"
            disabled={submitting}
          >
            {submitting ? t('question.sending') : t('question.answer')}
          </button>
        </form>
      )}
      {error ? (
        <p className="field-error" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
