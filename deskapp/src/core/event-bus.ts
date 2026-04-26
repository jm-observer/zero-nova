/**
 * 轻量级发布/订阅系统，用于解耦 UI 组件
 */

 export type EventHandler<T = any> = (payload: T) => void;

 export class EventBus {
   private handlers = new Map<string, Set<EventHandler>>();
 
   /**
    * 订阅事件
    * @returns 返回一个取消订阅的函数
    */
   on<T>(event: string, handler: EventHandler<T>): () => void {
     if (!this.handlers.has(event)) {
       this.handlers.set(event, new Set());
     }
     this.handlers.get(event)!.add(handler);
 
     return () => {
       const handlers = this.handlers.get(event);
       if (handlers) {
         handlers.delete(handler);
       }
     };
   }
 
   /**
    * 发布事件
    */
   emit<T>(event: string, payload?: T): void {
     const handlers = this.handlers.get(event);
     if (handlers) {
       handlers.forEach(handler => {
         try {
           handler(payload);
         } catch (e) {
           console.error(`Error in event handler for ${event}:`, e);
         }
       });
     }
   }
 
   /**
    * 清除所有事件监听（通常在应用卸载或重置时使用）
    */
   clear(): void {
     this.handlers.clear();
   }
 }
 
 // 预定义事件常量，避免拼写错误
 export const Events = {
   SESSION_SELECTED: 'session:selected',   // { sessionId: string }
   SESSION_CHANGED: 'session:changed',     // { sessionId: string, messages: any[] }
   SESSION_CREATED: 'session:created',     // { session: any }
   SESSION_DELETED: 'session:deleted',     // { sessionId: string }
   SESSION_UPDATED: 'session:updated',     // { sessions: any[] }
   SESSION_CREATE: 'session:create',       // { title?: string }
   SESSION_DELETE: 'session:delete',       // { id: string }
    SESSION_COPY: 'session:copy',           // { id: string, index?: number }
   MESSAGE_ADDED: 'message:added',         // { sessionId: string, message: any }
   MESSAGES_UPDATED: 'messages:updated',   // { sessionId: string, messages: any[] }
   AGENT_SWITCHED: 'agent:switched',       // { agentId: string }
   STREAMING_START: 'streaming:start',     // { sessionId: string }
   STREAMING_TOKEN: 'streaming:token',     // { token: string }
   STREAMING_END: 'streaming:end',         // { sessionId: string, message: any }
   SETTINGS_TOGGLE: 'settings:toggle',     // { visible: boolean }
   THEME_CHANGED: 'theme:changed',         // { theme: string }
   NOTIFICATION: 'notification',           // { type: 'success'|'error', message: string }
   VOICE_MODE_TOGGLE: 'voice:toggle',      // { active: boolean }
   PROGRESS_UPDATE: 'progress:update',     // { event: any }
   GATEWAY_STATUS: 'gateway:status',       // { status: 'connecting' | 'connected' | 'disconnected' | 'reconnecting' | 'failed' }
   CHAT_INTENT: 'chat:intent',             // { sessionId: string, intent: string, agentId?: string }
   CONSOLE_TOGGLED: 'console:toggled',     // { visible: boolean }
   CONSOLE_TAB_CHANGED: 'console:tab',     // { tab: string }
   CONSOLE_DATA_UPDATED: 'console:data',   // { key: string, data: any }
   CONSOLE_RUNTIME_UPDATED: 'console:runtime_updated',
   CONSOLE_TOKEN_UPDATED: 'console:token_updated',
   CONSOLE_TOOLS_UPDATED: 'console:tools_updated',
   CONSOLE_SKILLS_UPDATED: 'console:skills_updated',
   CONSOLE_MEMORY_UPDATED: 'console:memory_updated',
   SETTINGS_NAVIGATE: 'settings:navigate',
 } as const;
