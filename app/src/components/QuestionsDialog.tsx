import type { Question } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';
import { QuestionForm } from './QuestionForm';

interface QuestionsDialogProps {
  questions: Question[];
  onAnswer: (id: string, answer: string) => Promise<void>;
  onClose: () => void;
}

export function QuestionsDialog({
  questions,
  onAnswer,
  onClose,
}: QuestionsDialogProps) {
  const { t } = useLanguage();
  return (
    <Dialog
      titleId="questions-title"
      descriptionId="questions-description"
      className="questions-dialog"
      onClose={onClose}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="questions-title">
          {t('question.questions')}
        </h2>
        <p className="dialog-sub" id="questions-description">
          {questions.length === 0
            ? t('question.noneWaiting')
            : questions.length === 1
              ? t('question.oneWaiting')
              : t('question.manyWaiting', { count: questions.length })}
        </p>
      </div>
      <div className="dialog-body questions-body">
        {questions.length === 0 ? (
          <p className="empty-text">{t('question.nonePending')}</p>
        ) : (
          questions.map((question, index) => (
            <section className="question-card" key={question.id}>
              <span className="question-session">{question.sessionName}</span>
              <QuestionForm
                question={question}
                autoFocus={index === 0}
                primaryOptions
                onAnswer={(answer) => onAnswer(question.id, answer)}
              />
            </section>
          ))
        )}
      </div>
      <div className="dialog-foot">
        <button type="button" className="btn btn-ghost" onClick={onClose}>
          {t('common.close')}
        </button>
      </div>
    </Dialog>
  );
}
