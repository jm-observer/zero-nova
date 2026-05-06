const fs = require('fs');
const path = 'D:\\git\\zero-nova\\deskapp\\src\\services\\chat-service.ts';
let content = fs.readFileSync(path, 'utf8');

const oldText = `this.bus.emit('chat:complete', event);`;
const newText = `this.bus.emit('chat:complete', event);
            // 捕获本次请求的 token 使用量，关联到最新一条 assistant 消息
            if (event.usage && event.sessionId) {
                this.attachTokenUsageToLatestAssistantMessage(event.usage);
            }`;

if (content.includes(oldText)) {
    content = content.replace(oldText, newText);
    fs.writeFileSync(path, content, 'utf8');
    console.log('Updated chat-service.ts - added token usage capture');
} else {
    console.log('Pattern not found');
}
