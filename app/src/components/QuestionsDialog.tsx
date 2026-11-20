import type { Question } from '../api/types';
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
  return (
    <Dialog
      titleId="questions-title"
      descriptionId="questions-description"
      className="questions-dialog"
      onClose={onClose}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="questions-title">
          Questions
        </h2>
        <p className="dialog-sub" id="questions-description">
          {questions.length === 0
            ? 'No session is waiting for an answer.'
            : questions.length === 1
              ? 'One session is waiting for an answer.'
              : `${questions.length} sessions are waiting for an answer.`}
        </p>
      </div>
      <div className="dialog-body questions-body">
        {questions.length === 0 ? (
          <p className="empty-text">No pending questions.</p>
        ) : (
          questions.map((question) => (
            <section className="question-card" key={question.id}>
              <span className="question-session">{question.sessionName}</span>
              <QuestionForm
                question={question}
                onAnswer={(answer) => onAnswer(question.id, answer)}
              />
            </section>
          ))
        )}
      </div>
      <div className="dialog-foot">
        <button type="button" className="btn btn-ghost" onClick={onClose} autoFocus>
          Close
        </button>
      </div>
    </Dialog>
  );
}
