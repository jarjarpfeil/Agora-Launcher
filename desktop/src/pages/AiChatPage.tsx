import { AiAssistant } from '../components/AiAssistant';

export default function AiChatPage() {
  return (
    <div className="h-full overflow-hidden p-4">
      <div className="h-full max-w-3xl mx-auto">
        <AiAssistant onClose={() => {}} />
      </div>
    </div>
  );
}
