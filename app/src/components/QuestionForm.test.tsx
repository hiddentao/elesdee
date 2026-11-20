import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Question } from '../api/types';
import { QuestionForm } from './QuestionForm';

const question: Question = {
  id: 'q1',
  sessionId: 's1',
  sessionName: 'Fix login',
  text: 'Deploy now?',
  options: [],
  status: 'pending',
  answer: null,
  createdAt: '2026-01-01T00:00:00Z',
};

describe('QuestionForm', () => {
  it('answers with typed text', async () => {
    const user = userEvent.setup();
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(<QuestionForm question={question} onAnswer={onAnswer} />);

    await user.type(screen.getByLabelText('Answer'), 'Yes, deploy');
    await user.click(screen.getByRole('button', { name: 'Answer' }));

    expect(onAnswer).toHaveBeenCalledWith('Yes, deploy');
  });

  it('answers with a supplied option', async () => {
    const user = userEvent.setup();
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(
      <QuestionForm
        question={{ ...question, options: ['Yes', 'No'] }}
        onAnswer={onAnswer}
      />,
    );

    await user.click(screen.getByRole('button', { name: 'No' }));
    expect(onAnswer).toHaveBeenCalledWith('No');
  });

  it('keeps repeated option text as separate buttons', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const user = userEvent.setup();
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(
      <QuestionForm
        question={{ ...question, options: ['Red', 'Red'] }}
        onAnswer={onAnswer}
      />,
    );

    const options = screen.getAllByRole('button', { name: 'Red' });
    expect(options).toHaveLength(2);
    await user.click(options[1]);
    expect(onAnswer).toHaveBeenCalledWith('Red');
    expect(
      error.mock.calls.some((call) =>
        String(call[0]).includes('same key'),
      ),
    ).toBe(false);
    error.mockRestore();
  });

  it('does not send an empty answer', async () => {
    const user = userEvent.setup();
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(<QuestionForm question={question} onAnswer={onAnswer} />);

    await user.click(screen.getByRole('button', { name: 'Answer' }));
    expect(onAnswer).not.toHaveBeenCalled();
    expect(screen.getByText('Enter an answer.')).toBeInTheDocument();
  });
});
